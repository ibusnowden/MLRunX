# Policy Gradient Training Loop
#
# Original implementation by Zafir Stojanovski (@zafstojano)
# Source: https://github.com/zafstojano/policy-gradients
# License: Apache 2.0
#
# Adapted for RLHF Book (https://rlhfbook.com) by Nathan Lambert
# - Added SDPA fallback for platforms without flash-attn (e.g., DGX Spark)

import argparse
import os
import platform
import random
import re
import time
from dataclasses import dataclass
from datetime import timedelta
from itertools import batched  # Requires Python 3.12+

import mlrunx
import numpy as np
import reasoning_gym as rg
import torch
import torch.distributed as dist
import torch.nn as nn
import torch.nn.functional as F
import torch.optim as optim
from reasoning_gym.composite import DatasetSpec
from reasoning_gym.dataset import ProceduralDataset
from reasoning_gym.utils import SYSTEM_PROMPTS, extract_answer
from rich.console import Console
from torch.nn.parallel import DistributedDataParallel
from torch.nn.utils import clip_grad_norm_
from torch.utils.data import DataLoader
from torch.utils.data.distributed import DistributedSampler
from transformers import AutoConfig, AutoModelForCausalLM, AutoTokenizer, GenerationConfig
from transformers.utils import is_flash_attn_2_available, is_flash_attn_3_available

from .buffer import Experience, ReplayBuffer, join_experiences_batch
from .config import Config, load_config
from .loss import CISPOLoss, GRPOLoss, GSPOLoss, PPOLoss, ReinforceLoss, approx_kl, masked_mean
from .utils import print_model_info, print_step_summary, progress_bar


@dataclass(frozen=True)
class DistEnv:
    rank: int = 0
    local_rank: int = 0
    world_size: int = 1
    enabled: bool = False

    @property
    def is_main_process(self) -> bool:
        return self.rank == 0


def setup_distributed() -> DistEnv:
    """Initialize process group from torchrun environment (if enabled)."""
    world_size = int(os.environ.get("WORLD_SIZE", "1"))
    rank = int(os.environ.get("RANK", "0"))
    local_rank = int(os.environ.get("LOCAL_RANK", "0"))
    enabled = world_size > 1

    if enabled and not dist.is_initialized():
        backend = "nccl" if torch.cuda.is_available() else "gloo"
        dist.init_process_group(
            backend=backend,
            init_method="env://",
            timeout=timedelta(minutes=30),
        )

    if enabled and torch.cuda.is_available():
        torch.cuda.set_device(local_rank)

    return DistEnv(rank=rank, local_rank=local_rank, world_size=world_size, enabled=enabled)


def cleanup_distributed(dist_env: DistEnv) -> None:
    """Tear down process group after training."""
    if dist_env.enabled and dist.is_initialized():
        dist.destroy_process_group()


def unwrap_model(model):
    """Return the underlying model when wrapped in DDP."""
    if isinstance(model, DistributedDataParallel):
        return model.module
    return model


def get_model_device(model) -> torch.device:
    """Infer model device from parameters (works for plain and DDP models)."""
    return next(unwrap_model(model).parameters()).device


def _to_scalar(value: float | torch.Tensor) -> float:
    """Convert scalar-like values to Python float."""
    if isinstance(value, torch.Tensor):
        return value.detach().to(torch.float32).item()
    return float(value)


def distributed_sum(value: float | torch.Tensor, device: torch.device, dist_env: DistEnv) -> float:
    """Compute scalar sum across all ranks."""
    reduced = torch.tensor(_to_scalar(value), dtype=torch.float32, device=device)
    if dist_env.enabled:
        dist.all_reduce(reduced, op=dist.ReduceOp.SUM)
    return reduced.item()


def distributed_mean(value: float | torch.Tensor, device: torch.device, dist_env: DistEnv) -> float:
    """Compute mean scalar across all ranks."""
    total = distributed_sum(value, device, dist_env)
    if dist_env.enabled:
        return total / dist_env.world_size
    return total


def distributed_max(value: float | torch.Tensor, device: torch.device, dist_env: DistEnv) -> float:
    """Compute scalar max across all ranks."""
    reduced = torch.tensor(_to_scalar(value), dtype=torch.float32, device=device)
    if dist_env.enabled:
        dist.all_reduce(reduced, op=dist.ReduceOp.MAX)
    return reduced.item()


def filter_numeric_metrics(metrics: dict[str, float]) -> dict[str, float]:
    """Keep only finite numeric values for logging."""
    return {k: float(v) for k, v in metrics.items() if isinstance(v, (int, float)) and np.isfinite(v)}


def get_gpu_metrics() -> dict[str, float]:
    """Collect GPU metrics for MLRunX plotting (best-effort, no hard dependency on NVML)."""
    metrics: dict[str, float] = {}
    if not torch.cuda.is_available():
        return metrics

    device_count = torch.cuda.device_count()
    metrics["gpu/device_count"] = float(device_count)

    nvml = None
    try:
        import pynvml  # type: ignore

        pynvml.nvmlInit()
        nvml = pynvml
    except Exception:
        nvml = None

    for idx in range(device_count):
        prefix = f"gpu/{idx}" if device_count > 1 else "gpu"
        total_gb = torch.cuda.get_device_properties(idx).total_memory / (1024**3)
        allocated_gb = torch.cuda.memory_allocated(idx) / (1024**3)
        reserved_gb = torch.cuda.memory_reserved(idx) / (1024**3)
        max_allocated_gb = torch.cuda.max_memory_allocated(idx) / (1024**3)
        max_reserved_gb = torch.cuda.max_memory_reserved(idx) / (1024**3)

        metrics[f"{prefix}/memory_total_gb"] = total_gb
        metrics[f"{prefix}/memory_allocated_gb"] = allocated_gb
        metrics[f"{prefix}/memory_reserved_gb"] = reserved_gb
        metrics[f"{prefix}/memory_max_allocated_gb"] = max_allocated_gb
        metrics[f"{prefix}/memory_max_reserved_gb"] = max_reserved_gb
        metrics[f"{prefix}/memory_used_percent"] = (allocated_gb / total_gb) * 100.0 if total_gb > 0 else 0.0

        if nvml is not None:
            try:
                handle = nvml.nvmlDeviceGetHandleByIndex(idx)
                util = nvml.nvmlDeviceGetUtilizationRates(handle)
                metrics[f"{prefix}/utilization_percent"] = float(util.gpu)
                metrics[f"{prefix}/memory_utilization_percent"] = float(util.memory)
                metrics[f"{prefix}/temperature_c"] = float(
                    nvml.nvmlDeviceGetTemperature(handle, nvml.NVML_TEMPERATURE_GPU)
                )
                try:
                    metrics[f"{prefix}/power_w"] = float(nvml.nvmlDeviceGetPowerUsage(handle) / 1000.0)
                except Exception:
                    pass
            except Exception:
                pass

    if nvml is not None:
        try:
            nvml.nvmlShutdown()
        except Exception:
            pass

    return metrics


def get_attn_implementation() -> str:
    """Determine the best attention implementation for this platform.

    Priority order:
    1) flash_attention_3 (if available),
    2) flash_attention_2 (if available),
    3) sdpa fallback.
    """
    if platform.machine() != "x86_64":
        return "sdpa"  # aarch64 / DGX Spark - use SDPA with cuDNN

    if is_flash_attn_3_available():
        return "flash_attention_3"
    if is_flash_attn_2_available():
        return "flash_attention_2"
    return "sdpa"


def seed_everything(seed: int) -> None:
    """Set all random seeds for reproducibility."""
    random.seed(seed)
    os.environ["PYTHONHASHSEED"] = str(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)
    torch.backends.cudnn.deterministic = True
    torch.backends.cudnn.benchmark = False


def load_model(model_name: str, device: torch.device, gradient_checkpointing: bool = True):
    """Load model and tokenizer with automatic attention implementation selection."""
    attn_impl = get_attn_implementation()
    tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=False)
    model_config = AutoConfig.from_pretrained(model_name, trust_remote_code=False)
    if hasattr(model_config, "tie_word_embeddings"):
        model_config.tie_word_embeddings = False
    # Many decoder-only models (LLaMA, GPT-2) don't define pad_token
    # Set it to eos_token to enable batch padding
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(
        model_name,
        config=model_config,
        trust_remote_code=False,
        attn_implementation=attn_impl,
        dtype=torch.bfloat16,
    )
    model = model.to(device)
    if gradient_checkpointing:
        model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
    return model, tokenizer


def get_ref_model(model_name: str, device: torch.device, beta: float):
    """Load reference model for KL penalty (only if beta > 0)."""
    if not beta:
        return None
    ref_model, _ = load_model(model_name, device, gradient_checkpointing=False)
    ref_model.eval()
    return ref_model


def get_val_model(model_name: str, device: torch.device, loss: str, gradient_checkpointing: bool = True):
    """Load value model for PPO (only if loss == 'ppo')."""
    if loss not in ["ppo"]:
        return None
    val_model, _ = load_model(model_name, device, gradient_checkpointing)
    val_device = next(val_model.parameters()).device
    val_model.lm_head = nn.Linear(
        val_model.lm_head.in_features, 1, bias=False, device=val_device, dtype=torch.bfloat16
    )
    return val_model


def get_loss_objective(loss: str, **kwargs) -> nn.Module:
    """Get the loss function module for the specified algorithm."""
    if loss in ["grpo", "drgrpo"]:
        return GRPOLoss(**kwargs)
    elif loss == "gspo":
        return GSPOLoss(**kwargs)
    elif loss in ["rloo", "reinforce"]:
        return ReinforceLoss(**kwargs)
    elif loss == "cispo":
        return CISPOLoss(**kwargs)
    elif loss == "ppo":
        return PPOLoss(**kwargs)
    raise ValueError(f"Unsupported loss type: {loss}")


def _accuracy_reward(dataset: ProceduralDataset, completions: str, entries: list[dict]) -> float:
    """Compute accuracy reward based on extracted answers."""

    def score_answer(completion: str, entry: dict) -> float:
        answer = extract_answer(completion)
        return dataset.score_answer(answer, entry)

    return [score_answer(c, e) for c, e in zip(completions, entries, strict=True)]


def _format_reward(completions: list[str], **kwargs) -> list[float]:
    """Compute format reward based on presence of thinking/answer tags."""

    def count_tags(text: str) -> float:
        count = 0.0
        if re.search(r"\s*<think>\s*", text):
            count += 0.25
        if re.search(r"\s*</think>\s*", text):
            count += 0.25
        if re.search(r"\s*<answer>\s*", text):
            count += 0.25
        if re.search(r"\s*</answer>\s*", text):
            count += 0.25
        return count

    return [count_tags(c) for c in completions]


def compute_rewards(
    dataset: ProceduralDataset, completions: list[str], entries: list[dict], format_weight: float = 0.5
) -> list[float]:
    """Compute combined accuracy + format rewards."""
    accuracy_rewards = _accuracy_reward(dataset, completions, entries)
    format_rewards = _format_reward(completions)
    combined_rewards = [acc + format_weight * fmt for acc, fmt in zip(accuracy_rewards, format_rewards, strict=True)]
    return combined_rewards


def apply_reward_kl(
    rewards: torch.Tensor,
    log_probs: torch.Tensor,
    log_probs_ref: torch.Tensor,
    action_mask: torch.Tensor,
    beta: float,
    loss: str,
) -> torch.Tensor:
    """Apply KL penalty to rewards (for REINFORCE/RLOO/PPO)."""
    if not beta or loss not in ["ppo", "rloo", "reinforce"]:
        return rewards
    kl_div = approx_kl(log_probs, log_probs_ref, action_mask)
    kl_div = masked_mean(kl_div, mask=action_mask, dim=-1, keepdim=True)
    rewards = rewards - beta * kl_div
    return rewards


def compute_standardized_advantages(rewards: torch.Tensor, eps: float = 1e-8) -> torch.Tensor:
    """Compute standardized advantages (GRPO, GSPO, CISPO)."""
    return (rewards - rewards.mean(dim=0, keepdim=True)) / (rewards.std(dim=0, keepdim=True) + eps)


def compute_nonstandardized_advantages(rewards: torch.Tensor) -> torch.Tensor:
    """Compute non-standardized advantages (Dr. GRPO)."""
    return rewards - rewards.mean(dim=0, keepdim=True)


def compute_loo_advantages(rewards: torch.Tensor) -> torch.Tensor:
    """Compute leave-one-out advantages (RLOO)."""
    K = rewards.shape[0]
    return (K / (K - 1)) * (rewards - rewards.mean(dim=0, keepdim=True))


def compute_gae(
    rewards: torch.Tensor, action_mask: torch.Tensor, values: torch.Tensor, gamma: float, lam: float
) -> torch.Tensor:
    """Compute Generalized Advantage Estimation (PPO)."""
    B, S = action_mask.size()
    device = action_mask.device
    last_action_indices = action_mask.long().cumsum(dim=-1).argmax(dim=-1, keepdim=True)
    indices = torch.arange(S, device=device).unsqueeze(0)
    done = (indices >= last_action_indices).float()

    rewards = torch.zeros_like(action_mask, device=device, dtype=torch.float32).scatter_(
        dim=-1, index=last_action_indices, src=rewards
    )

    values = values.to(device)
    advantages = torch.zeros_like(action_mask, dtype=torch.float32, device=device)
    next_values = torch.zeros(B, device=device, dtype=torch.float32)
    running = torch.zeros(B, device=device, dtype=torch.float32)

    for t in reversed(range(S)):
        not_done = 1.0 - done[:, t]
        delta = rewards[:, t] + not_done * gamma * next_values - values[:, t]
        running = delta + not_done * gamma * lam * running
        advantages[:, t] = running
        next_values = values[:, t]

    advantages = advantages * action_mask
    return advantages


def compute_advantages(
    rewards: torch.Tensor,
    loss: str,
    action_mask: torch.Tensor | None = None,
    values: torch.Tensor | None = None,
    gamma: float | None = None,
    lam: float | None = None,
) -> torch.Tensor:
    """Compute advantages using the appropriate method for the loss function."""
    if loss in ["grpo", "gspo", "cispo"]:
        return compute_standardized_advantages(rewards)
    elif loss in ["drgrpo"]:
        return compute_nonstandardized_advantages(rewards)
    elif loss in ["rloo"]:
        return compute_loo_advantages(rewards)
    elif loss in ["ppo"]:
        return compute_gae(rewards, action_mask, values, gamma, lam)
    else:
        return rewards


def compute_log_probs(model, sequence_ids: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
    """Compute log probabilities for each token in the sequence."""
    if not model:
        return None
    model_device = get_model_device(model)
    sequence_ids, attention_mask = sequence_ids.to(model_device), attention_mask.to(model_device)
    output = model(input_ids=sequence_ids, attention_mask=attention_mask, use_cache=False)
    logits = output.logits[:, :-1, :].to(torch.float32)
    log_probs = F.log_softmax(logits, dim=-1)
    targets = sequence_ids[:, 1:].unsqueeze(-1)
    target_log_probs = torch.gather(log_probs, dim=-1, index=targets).squeeze(-1)
    return target_log_probs


def compute_values(model, sequence_ids: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
    """Compute value estimates for each position (PPO)."""
    if not model:
        return None
    model_device = get_model_device(model)
    sequence_ids, attention_mask = sequence_ids.to(model_device), attention_mask.to(model_device)
    output = model(input_ids=sequence_ids, attention_mask=attention_mask, use_cache=False)
    values = output.logits[:, :-1, :].squeeze(-1).to(torch.float32)
    return values


def rollout(
    model,
    entries: list[dict],
    dataset: ProceduralDataset,
    tokenizer: AutoTokenizer,
    max_new_tokens: int,
    temperature: float,
    top_p: float,
    top_k: int,
    min_p: float,
):
    """Generate completions and compute rewards."""
    model_device = get_model_device(model)
    generation_model = unwrap_model(model)

    # 1. Format prompts
    message_templates = [
        tokenizer.apply_chat_template(
            [
                {"role": "system", "content": SYSTEM_PROMPTS["DeepSeekZero"]},
                {"role": "user", "content": entry["question"]},
            ],
            tokenize=False,
            add_generation_prompt=True,
            enable_thinking=True,
        )
        for entry in entries
    ]
    model_inputs = tokenizer(
        message_templates,
        return_tensors="pt",
        padding=True,
        padding_side="left",
        return_attention_mask=True,
    ).to(model_device)

    # 2. Generate responses
    pad_token_id = tokenizer.pad_token_id if tokenizer.pad_token_id is not None else tokenizer.eos_token_id
    generation_config = GenerationConfig(
        temperature=temperature,
        top_p=top_p,
        top_k=top_k,
        min_p=min_p,
        do_sample=True,
        max_new_tokens=max_new_tokens,
        pad_token_id=pad_token_id,
    )
    sequence_ids = generation_model.generate(**model_inputs, generation_config=generation_config)
    completion_ids = sequence_ids[:, model_inputs["input_ids"].shape[1] :]
    completions = tokenizer.batch_decode(completion_ids, skip_special_tokens=True)

    # 3. Obtain the generated tokens only
    action_mask = torch.zeros_like(sequence_ids, dtype=torch.bool)
    action_mask[:, model_inputs["input_ids"].shape[1] :] = True
    action_mask[sequence_ids == pad_token_id] = False
    action_mask = action_mask[:, 1:]

    # 4. Compute rewards
    rewards = compute_rewards(dataset, completions, entries)
    rewards = torch.tensor(rewards, dtype=torch.float32, device=model_device).unsqueeze(-1)

    # 5. Compute attention mask
    attention_mask = sequence_ids != tokenizer.pad_token_id

    return sequence_ids, action_mask, attention_mask, rewards, completions


def create_dataset(cfg: Config) -> ProceduralDataset:
    """Create the training dataset from config."""
    specs = [DatasetSpec(name=s.name, weight=s.weight, config=s.config) for s in cfg.data.specs]
    return rg.create_dataset("composite", size=cfg.data.size, seed=cfg.seed, datasets=specs)


def main(cfg: Config):
    """Main training loop."""
    dist_env = setup_distributed()
    seed_everything(cfg.seed + dist_env.rank)
    console = Console()
    show_progress = dist_env.is_main_process and console.is_terminal

    # Print attention implementation info
    attn_impl = get_attn_implementation()
    if dist_env.is_main_process:
        console.print(f"[dim]Using attention implementation: {attn_impl}[/dim]")
        if dist_env.enabled:
            console.print(
                f"[dim]torchrun detected: world_size={dist_env.world_size}, rank={dist_env.rank},"
                f" local_rank={dist_env.local_rank}[/dim]"
            )

    cpu_device = torch.device("cpu")
    if torch.cuda.is_available():
        if dist_env.enabled:
            model_device = ref_model_device = val_model_device = torch.device(f"cuda:{dist_env.local_rank}")
            if (
                dist_env.is_main_process
                and (cfg.model_device_id != 0 or cfg.ref_model_device_id != 0 or cfg.val_model_device_id != 0)
            ):
                console.print(
                    "[yellow]Distributed mode ignores model_device_id/ref_model_device_id/val_model_device_id and"
                    " uses LOCAL_RANK for each process.[/yellow]"
                )
        else:
            model_device = torch.device(f"cuda:{cfg.model_device_id}")
            ref_model_device = torch.device(f"cuda:{cfg.ref_model_device_id}")
            val_model_device = torch.device(f"cuda:{cfg.val_model_device_id}")
    else:
        model_device = ref_model_device = val_model_device = cpu_device

    dataset = create_dataset(cfg)
    sampler = (
        DistributedSampler(
            dataset,
            num_replicas=dist_env.world_size,
            rank=dist_env.rank,
            shuffle=True,
            drop_last=True,
        )
        if dist_env.enabled
        else None
    )
    dataloader = DataLoader(
        dataset=dataset,
        batch_size=cfg.prompts_per_step,
        shuffle=sampler is None,
        sampler=sampler,
        pin_memory=False,
        drop_last=True,
        collate_fn=lambda x: x,
    )
    if sampler is not None:
        sampler.set_epoch(0)

    try:
        model, tokenizer = load_model(cfg.model_name, model_device, gradient_checkpointing=True)
        if dist_env.enabled:
            if model_device.type == "cuda":
                model = DistributedDataParallel(
                    model,
                    device_ids=[model_device.index],
                    output_device=model_device.index,
                    find_unused_parameters=False,
                )
            else:
                model = DistributedDataParallel(model, find_unused_parameters=False)

        ref_model = get_ref_model(cfg.model_name, ref_model_device, cfg.beta)
        val_model = get_val_model(cfg.model_name, val_model_device, cfg.loss, gradient_checkpointing=True)
        if val_model and dist_env.enabled:
            if val_model_device.type == "cuda":
                val_model = DistributedDataParallel(
                    val_model,
                    device_ids=[val_model_device.index],
                    output_device=val_model_device.index,
                    find_unused_parameters=False,
                )
            else:
                val_model = DistributedDataParallel(val_model, find_unused_parameters=False)

        objective = get_loss_objective(
            loss=cfg.loss,
            clip_eps_lo=cfg.clip_eps_lo,
            clip_eps_hi=cfg.clip_eps_hi,
            clip_eps_val=cfg.clip_eps_val,
            vf_coef=cfg.vf_coef,
            beta=cfg.beta,
        ).to(get_model_device(model))
        params = list(model.parameters()) + (list(val_model.parameters()) if val_model else [])
        optimizer = optim.Adam(params, lr=cfg.lr)
        replay_buffer = ReplayBuffer()

        # Initialize MLRunX for experiment tracking on rank 0 only
        run = None
        if dist_env.is_main_process:
            run_name = cfg.mlrunx_run_name or f"{cfg.loss}_seed{cfg.seed}"
            run = mlrunx.init(
                project=cfg.mlrunx_project,
                name=run_name,
                tags={
                    "loss": cfg.loss,
                    "model": cfg.model_name,
                    "seed": str(cfg.seed),
                },
            )
            run.log_params({
                "loss": cfg.loss,
                "model_name": cfg.model_name,
                "lr": cfg.lr,
                "beta": cfg.beta,
                "clip_eps_lo": cfg.clip_eps_lo,
                "clip_eps_hi": cfg.clip_eps_hi,
                "temperature": cfg.temperature,
                "top_p": cfg.top_p,
                "top_k": cfg.top_k,
                "max_new_tokens": cfg.max_new_tokens,
                "prompts_per_step": cfg.prompts_per_step,
                "num_rollouts": cfg.num_rollouts,
                "train_batch_size": cfg.train_batch_size,
                "batch_acc": cfg.batch_acc,
                "data_size": cfg.data.size,
                "world_size": dist_env.world_size,
            })
            print_model_info(console, unwrap_model(model))

        start_time = time.time()
        for step, batch in enumerate(dataloader):
            step_start_time = time.time()
            model.eval()
            if val_model:
                val_model.eval()
            replay_buffer.clear()
            rollout_rewards = []
            rollout_tokens_local = 0.0
            rollout_samples_local = 0.0
            rollout_start_time = time.time()

            with progress_bar(console, disable=not show_progress) as progress:
                entries = [entry for entry in batch for _ in range(cfg.num_rollouts)]
                task = progress.add_task("Generating rollouts", total=len(entries) // cfg.rollout_batch_size)

                for rollout_batch in batched(entries, cfg.rollout_batch_size):
                    with torch.no_grad():
                        sequence_ids, action_mask, attention_mask, rewards, completions = rollout(
                            model=model,
                            entries=rollout_batch,
                            dataset=dataset,
                            tokenizer=tokenizer,
                            max_new_tokens=cfg.max_new_tokens,
                            temperature=cfg.temperature,
                            top_p=cfg.top_p,
                            top_k=cfg.top_k,
                            min_p=cfg.min_p,
                        )
                        rollout_rewards.append(rewards.cpu())
                        rollout_tokens_local += float(action_mask.sum().item())
                        rollout_samples_local += float(action_mask.size(0))

                        log_probs_old = compute_log_probs(model, sequence_ids, attention_mask)
                        log_probs_ref = compute_log_probs(ref_model, sequence_ids, attention_mask)
                        values_old = compute_values(val_model, sequence_ids, attention_mask)
                        rewards = apply_reward_kl(rewards, log_probs_old, log_probs_ref, action_mask, cfg.beta, cfg.loss)
                        advantages = compute_advantages(rewards, cfg.loss, action_mask, values_old, cfg.gamma, cfg.lam)

                        experience = Experience(
                            sequence_ids=sequence_ids,
                            attention_mask=attention_mask,
                            action_mask=action_mask,
                            advantages=advantages,
                            log_probs_old=log_probs_old,
                            log_probs_ref=log_probs_ref,
                            values_old=values_old,
                        ).to(cpu_device)
                        replay_buffer.add(experience)

                    progress.update(task, advance=1)

            # Rollout-level metrics
            device = get_model_device(model)
            avg_reward_local = torch.cat(rollout_rewards, dim=0).mean().item()
            avg_reward = distributed_mean(avg_reward_local, device, dist_env)

            rollout_tokens = distributed_sum(rollout_tokens_local, device, dist_env)
            rollout_samples = distributed_sum(rollout_samples_local, device, dist_env)
            rollout_time_local = max(time.time() - rollout_start_time, 1e-6)
            rollout_time_s = distributed_max(rollout_time_local, device, dist_env)
            throughput_toks_per_s = rollout_tokens / max(rollout_time_s, 1e-6)
            seq_len_toks_per_sample = rollout_tokens / max(rollout_samples, 1.0)

            torch.cuda.empty_cache()
            model.train()
            if val_model:
                val_model.train()

            experience_sampler = DataLoader(
                dataset=replay_buffer.buffer,
                batch_size=cfg.train_batch_size,
                shuffle=True,
                pin_memory=False,
                drop_last=True,
                collate_fn=join_experiences_batch,
            )

            # Step-aggregated training metrics
            step_loss_sum_local = 0.0
            grad_norm_sum_local = 0.0
            optimizer_updates_local = 0.0
            max_off_policy_level_local = 0.0

            with progress_bar(console, disable=not show_progress) as progress:
                task = progress.add_task("Training", total=len(experience_sampler))

                optimizer.zero_grad(set_to_none=True)
                accumulated_loss = 0.0

                for batch_idx, experience in enumerate(experience_sampler):
                    experience: Experience
                    experience = experience.to(device)

                    # Compute loss
                    log_probs = compute_log_probs(model, experience.sequence_ids, experience.attention_mask)
                    values = compute_values(val_model, experience.sequence_ids, experience.attention_mask)
                    loss = objective(log_probs=log_probs, experience=experience, values=values)

                    # Estimate how far we drift from rollout policy (off-policy level)
                    off_policy = approx_kl(log_probs, experience.log_probs_old, experience.action_mask)
                    off_policy_level = masked_mean(off_policy, mask=experience.action_mask, dim=-1).max().item()
                    max_off_policy_level_local = max(max_off_policy_level_local, off_policy_level)

                    is_finite = torch.isfinite(loss).to(device, dtype=torch.int32)
                    if dist_env.enabled:
                        dist.all_reduce(is_finite, op=dist.ReduceOp.MIN)
                    if is_finite.item() == 0:
                        continue
                    scaled_loss = loss / cfg.batch_acc
                    scaled_loss.backward()
                    accumulated_loss += loss.item()

                    # Update weights every batch_acc steps
                    if (batch_idx + 1) % cfg.batch_acc == 0 or (batch_idx + 1) == len(experience_sampler):
                        grad_norm = clip_grad_norm_(params, max_norm=cfg.max_norm)
                        optimizer.step()
                        optimizer.zero_grad(set_to_none=True)
                        torch.cuda.empty_cache()

                        num_accumulated = min(cfg.batch_acc, (batch_idx % cfg.batch_acc) + 1)
                        avg_loss_local = accumulated_loss / num_accumulated
                        grad_norm_local = _to_scalar(grad_norm)

                        step_loss_sum_local += avg_loss_local
                        grad_norm_sum_local += grad_norm_local
                        optimizer_updates_local += 1.0

                        progress.update(task, advance=1, description=f"[dim]Loss: {avg_loss_local:.4f}[/dim]")
                        accumulated_loss = 0.0
                    else:
                        progress.update(task, advance=1)

            optimizer_updates = distributed_sum(optimizer_updates_local, device, dist_env)
            step_loss_sum = distributed_sum(step_loss_sum_local, device, dist_env)
            grad_norm_sum = distributed_sum(grad_norm_sum_local, device, dist_env)
            max_off_policy_level = distributed_max(max_off_policy_level_local, device, dist_env)

            if optimizer_updates > 0:
                step_loss = step_loss_sum / optimizer_updates
                grad_norm_mean = grad_norm_sum / optimizer_updates
            else:
                step_loss = float("nan")
                grad_norm_mean = float("nan")

            async_level = optimizer_updates / dist_env.world_size if dist_env.world_size > 0 else 0.0
            step_time_local = max(time.time() - step_start_time, 1e-6)
            step_time_s = distributed_max(step_time_local, device, dist_env)
            hours_elapsed = (time.time() - start_time) / 3600

            if run is not None:
                step_metrics = {
                    "reward": avg_reward,
                    "time/hours_elapsed": hours_elapsed,
                    "time/step_seconds": step_time_s,
                    "throughput": throughput_toks_per_s,
                    "throughput/tokens_per_sec": throughput_toks_per_s,
                    "throughput/tokens_per_step": rollout_tokens,
                    "seq/length_tokens_per_sample": seq_len_toks_per_sample,
                    "async/level": async_level,
                    "off_policy/max_level": max_off_policy_level,
                    "updates/optimizer_steps": async_level,
                }
                if np.isfinite(step_loss):
                    step_metrics["loss"] = step_loss
                if np.isfinite(grad_norm_mean):
                    step_metrics["grad_norm"] = grad_norm_mean
                step_metrics.update(get_gpu_metrics())
                run.log(filter_numeric_metrics(step_metrics), step=step)

            if dist_env.is_main_process:
                print_step_summary(
                    console=console,
                    step=step,
                    total=len(dataloader),
                    step_time_s=step_time_s,
                    loss=step_loss,
                    reward=avg_reward,
                    throughput_toks_per_s=throughput_toks_per_s,
                    seq_len_toks_per_sample=seq_len_toks_per_sample,
                    async_level=async_level,
                    max_off_policy_level=max_off_policy_level,
                )

        # Finish MLRunX run
        if run is not None:
            run.finish()
            console.print(f"[green]Training complete! Run ID: {run.run_id}[/green]")
    finally:
        cleanup_distributed(dist_env)


def main_cli():
    """CLI entry point."""
    parser = argparse.ArgumentParser(description="Train policy gradient models for RLHF")
    parser.add_argument("--config", type=str, required=True, help="Path to YAML config file")
    args = parser.parse_args()
    cfg = load_config(args.config)
    main(cfg)


if __name__ == "__main__":
    main_cli()
