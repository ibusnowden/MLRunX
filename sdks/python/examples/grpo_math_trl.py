"""GRPO Math Training with TRL and MLRunX SDK.

This example demonstrates Group Relative Policy Optimization (GRPO) for
fine-tuning a language model on math problems using TRL's built-in GRPOTrainer.

Features demonstrated:
- GRPO training on GSM8K math dataset
- Qwen3 model with LoRA for CPU efficiency
- MLRunX integration for tracking rewards, loss, and metrics
- DeepSeek-style no-KL training for math

Requirements:
    pip install torch transformers datasets trl peft accelerate

Usage:
    # Quick test (CPU)
    python grpo_math_trl.py --quick

    # Full run (GPU)
    python grpo_math_trl.py --max-steps 500

Note: Works in offline mode if MLRunX server is not running.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
import time
from dataclasses import dataclass
from typing import Any

import mlrunx
from system_metrics import get_system_metrics, get_device_info

# Check dependencies
MISSING_DEPS = []
for pkg in ["torch", "transformers", "datasets", "trl", "peft"]:
    if importlib.util.find_spec(pkg) is None:
        MISSING_DEPS.append(pkg)

if MISSING_DEPS:
    print(f"Missing dependencies: {', '.join(MISSING_DEPS)}")
    print("Install with: pip install torch transformers datasets trl peft accelerate")
    sys.exit(1)

import torch
from datasets import load_dataset
from peft import LoraConfig, TaskType, get_peft_model
from transformers import (
    AutoModelForCausalLM,
    AutoTokenizer,
    TrainerCallback,
    TrainerControl,
    TrainerState,
    TrainingArguments,
)
from trl import GRPOConfig, GRPOTrainer


# =============================================================================
# Configuration
# =============================================================================

@dataclass
class GRPOExperimentConfig:
    """Configuration for GRPO experiment."""

    # Model
    model_name: str = "Qwen/Qwen3-0.6B"
    use_lora: bool = True
    lora_r: int = 8
    lora_alpha: int = 16
    lora_dropout: float = 0.05

    # Training
    max_steps: int = 10
    batch_size: int = 1
    gradient_accumulation_steps: int = 2
    learning_rate: float = 1e-5

    # GRPO specific
    num_generations: int = 2  # Rollouts per example
    max_new_tokens: int = 64
    max_length: int = 256

    # GRPO hyperparameters (DeepSeek math style)
    kl_coeff: float = 0.0  # No KL penalty for math
    clip_range: float = 0.2
    temperature: float = 0.7
    top_p: float = 0.9
    top_k: int = 50

    # Dataset
    num_train_samples: int = 100
    num_eval_samples: int = 20

    # Device
    device: str = "cpu"

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for logging."""
        return {
            "model_name": self.model_name,
            "use_lora": self.use_lora,
            "lora_r": self.lora_r,
            "lora_alpha": self.lora_alpha,
            "max_steps": self.max_steps,
            "batch_size": self.batch_size,
            "gradient_accumulation_steps": self.gradient_accumulation_steps,
            "learning_rate": self.learning_rate,
            "num_generations": self.num_generations,
            "max_new_tokens": self.max_new_tokens,
            "max_length": self.max_length,
            "kl_coeff": self.kl_coeff,
            "clip_range": self.clip_range,
            "temperature": self.temperature,
            "top_p": self.top_p,
            "top_k": self.top_k,
            "num_train_samples": self.num_train_samples,
            "num_eval_samples": self.num_eval_samples,
            "device": self.device,
        }


# =============================================================================
# Reward Function
# =============================================================================

def extract_answer(text: str) -> str | None:
    """Extract the final numerical answer from a completion.

    GSM8K answers are formatted as "#### <number>"
    """
    # Look for #### pattern
    match = re.search(r"####\s*(-?\d+(?:,\d{3})*(?:\.\d+)?)", text)
    if match:
        # Remove commas from numbers like 1,000
        return match.group(1).replace(",", "")

    # Fallback: look for last number in the text
    numbers = re.findall(r"-?\d+(?:\.\d+)?", text)
    if numbers:
        return numbers[-1]

    return None


def compute_reward(completions: list[str], ground_truth: str) -> list[float]:
    """Compute rewards for a batch of completions.

    Args:
        completions: List of model completions
        ground_truth: Expected answer (from GSM8K)

    Returns:
        List of rewards (1.0 for correct, 0.0 for incorrect)
    """
    expected = extract_answer(ground_truth)
    if expected is None:
        # Can't parse expected answer, give partial credit
        return [0.5] * len(completions)

    rewards = []
    for completion in completions:
        predicted = extract_answer(completion)
        if predicted is not None and predicted == expected:
            rewards.append(1.0)
        else:
            rewards.append(0.0)

    return rewards


# =============================================================================
# MLRunX Callback for GRPOTrainer
# =============================================================================

class MLRunXGRPOCallback(TrainerCallback):
    """Callback to log GRPO training metrics to MLRunX."""

    def __init__(self, run: mlrunx.Run):
        self.run = run
        self.step = 0
        self.rewards_history: list[float] = []
        self.correct_history: list[float] = []

    def on_log(
        self,
        args: TrainingArguments,
        state: TrainerState,
        control: TrainerControl,
        logs: dict[str, Any] | None = None,
        **kwargs: Any,
    ) -> None:
        """Log metrics from trainer."""
        if logs is None:
            return

        self.step = state.global_step

        # Log all metrics from trainer
        metrics_to_log = {}
        for key, value in logs.items():
            if isinstance(value, (int, float)):
                # Clean up metric names for MLRunX
                clean_key = key.replace("/", "_")
                metrics_to_log[clean_key] = value

        # Add system metrics (GPU, CPU, memory)
        metrics_to_log.update(get_system_metrics())

        if metrics_to_log:
            self.run.log(metrics_to_log, step=self.step)

    def log_rewards(self, rewards: list[float], correct_ratio: float) -> None:
        """Log reward statistics."""
        self.rewards_history.extend(rewards)
        self.correct_history.append(correct_ratio)

        self.run.log({
            "reward_mean": sum(rewards) / len(rewards) if rewards else 0,
            "reward_std": self._std(rewards),
            "reward_max": max(rewards) if rewards else 0,
            "reward_min": min(rewards) if rewards else 0,
            "correct_ratio": correct_ratio,
        }, step=self.step)

    @staticmethod
    def _std(values: list[float]) -> float:
        """Compute standard deviation."""
        if len(values) < 2:
            return 0.0
        mean = sum(values) / len(values)
        variance = sum((x - mean) ** 2 for x in values) / len(values)
        return variance ** 0.5


# =============================================================================
# Dataset Preparation
# =============================================================================

def prepare_gsm8k_dataset(
    tokenizer: AutoTokenizer,
    num_train: int = 100,
    num_eval: int = 20,
) -> tuple[list[dict], list[dict]]:
    """Load and prepare GSM8K dataset for GRPO training.

    Returns:
        Tuple of (train_data, eval_data) where each is a list of dicts
        with 'prompt' and 'answer' keys.
    """
    print("Loading GSM8K dataset...")
    dataset = load_dataset("openai/gsm8k", "main")

    def format_prompt(example: dict) -> dict:
        """Format a GSM8K example as a prompt."""
        question = example["question"]
        prompt = f"""Solve this math problem step by step. Show your work and end with the answer in the format "#### <number>".

Question: {question}

Solution:"""
        return {
            "prompt": prompt,
            "answer": example["answer"],
        }

    # Process train and test splits
    train_data = [
        format_prompt(dataset["train"][i])
        for i in range(min(num_train, len(dataset["train"])))
    ]

    eval_data = [
        format_prompt(dataset["test"][i])
        for i in range(min(num_eval, len(dataset["test"])))
    ]

    print(f"Prepared {len(train_data)} train, {len(eval_data)} eval examples")
    return train_data, eval_data


# =============================================================================
# Model Setup
# =============================================================================

def setup_model_and_tokenizer(
    config: GRPOExperimentConfig,
) -> tuple[AutoModelForCausalLM, AutoTokenizer]:
    """Load model and tokenizer with optional LoRA."""

    print(f"Loading model: {config.model_name}")

    # Load tokenizer
    tokenizer = AutoTokenizer.from_pretrained(
        config.model_name,
        trust_remote_code=True,
        padding_side="left",
    )
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    # Load model
    model = AutoModelForCausalLM.from_pretrained(
        config.model_name,
        trust_remote_code=True,
        torch_dtype=torch.float32 if config.device == "cpu" else torch.float16,
        device_map=config.device if config.device != "cpu" else None,
    )

    if config.device == "cpu":
        model = model.to("cpu")

    # Apply LoRA if enabled
    if config.use_lora:
        print("Applying LoRA configuration...")
        lora_config = LoraConfig(
            task_type=TaskType.CAUSAL_LM,
            r=config.lora_r,
            lora_alpha=config.lora_alpha,
            lora_dropout=config.lora_dropout,
            target_modules=["q_proj", "k_proj", "v_proj", "o_proj"],
        )
        model = get_peft_model(model, lora_config)
        model.print_trainable_parameters()

    return model, tokenizer


# =============================================================================
# Custom Reward Function for TRL
# =============================================================================

def create_reward_function(train_data: list[dict]):
    """Create a reward function that TRL's GRPOTrainer can use.

    TRL expects a function that takes (prompts, completions, **kwargs) and returns rewards.
    """
    # Create a mapping from prompt to expected answer
    prompt_to_answer = {item["prompt"]: item["answer"] for item in train_data}

    def reward_fn(
        prompts: list[str],
        completions: list[str],
        **kwargs: Any,
    ) -> list[float]:
        """Compute rewards for completions."""
        rewards = []
        for prompt, completion in zip(prompts, completions):
            expected_answer = prompt_to_answer.get(prompt, "")
            reward = compute_reward([completion], expected_answer)[0]
            rewards.append(reward)
        return rewards

    return reward_fn


# =============================================================================
# Main Training Function
# =============================================================================

def main():
    """Run GRPO training with MLRunX tracking."""

    parser = argparse.ArgumentParser(description="GRPO Math Training with TRL")
    parser.add_argument("--quick", action="store_true", help="Quick test run")
    parser.add_argument("--max-steps", type=int, default=None, help="Max training steps")
    parser.add_argument("--model", type=str, default="Qwen/Qwen3-0.6B", help="Model name")
    parser.add_argument("--device", type=str, default="cpu", help="Device (cpu/cuda)")
    args = parser.parse_args()

    # Configure experiment
    config = GRPOExperimentConfig(
        model_name=args.model,
        device=args.device,
    )

    if args.quick:
        config.max_steps = 5
        config.num_train_samples = 20
        config.num_eval_samples = 5
        config.num_generations = 2
        config.max_new_tokens = 32

    if args.max_steps is not None:
        config.max_steps = args.max_steps

    print("=" * 60)
    print("GRPO Math Training with TRL and MLRunX")
    print("=" * 60)
    print(f"\nConfiguration:")
    for key, value in config.to_dict().items():
        print(f"  {key}: {value}")
    print()

    # Initialize MLRunX
    run = mlrunx.init(
        project="grpo-math",
        name=f"grpo-trl-{config.model_name.split('/')[-1]}",
        tags={
            "framework": "trl",
            "task": "math",
            "dataset": "gsm8k",
            "model_family": "qwen3",
        },
    )
    print(f"MLRunX Run ID: {run.run_id}")
    print(f"Offline mode: {run.is_offline}")
    print()

    # Log configuration
    run.log_params(config.to_dict())

    # Log device/system info
    run.log_params(get_device_info())

    # Setup model and tokenizer
    model, tokenizer = setup_model_and_tokenizer(config)

    # Prepare dataset
    train_data, eval_data = prepare_gsm8k_dataset(
        tokenizer,
        num_train=config.num_train_samples,
        num_eval=config.num_eval_samples,
    )

    # Create reward function
    reward_fn = create_reward_function(train_data)

    # Create MLRunX callback
    mlrunx_callback = MLRunXGRPOCallback(run=run)

    # Configure GRPO
    grpo_config = GRPOConfig(
        output_dir="./grpo_output",

        # Training
        max_steps=config.max_steps,
        per_device_train_batch_size=config.batch_size,
        gradient_accumulation_steps=config.gradient_accumulation_steps,
        learning_rate=config.learning_rate,

        # GRPO specific
        num_generations=config.num_generations,
        max_completion_length=config.max_new_tokens,
        max_prompt_length=config.max_length,

        # DeepSeek math style - no KL (beta=0)
        beta=config.kl_coeff,  # KL coefficient
        epsilon=config.clip_range,  # PPO clip range

        # Generation
        temperature=config.temperature,
        top_p=config.top_p,
        top_k=config.top_k,

        # Loss type (dapo = DAPO-style active sampling)
        loss_type="grpo",

        # Logging
        logging_steps=1,
        report_to="none",  # We use MLRunX instead

        # Misc
        seed=42,
        remove_unused_columns=False,
    )

    # Prepare training data in TRL format
    # TRL expects a dataset with 'prompt' column
    from datasets import Dataset
    train_dataset = Dataset.from_list([{"prompt": d["prompt"]} for d in train_data])

    # Create trainer
    print("\nInitializing GRPOTrainer...")
    trainer = GRPOTrainer(
        model=model,
        args=grpo_config,
        train_dataset=train_dataset,
        processing_class=tokenizer,
        reward_funcs=reward_fn,
        callbacks=[mlrunx_callback],
    )

    # Train
    print("\nStarting GRPO training...")
    start_time = time.time()

    try:
        train_result = trainer.train()
        training_time = time.time() - start_time

        # Log final metrics
        run.log_params({
            "final/training_time_seconds": training_time,
            "final/total_steps": trainer.state.global_step,
        })

        if hasattr(train_result, "metrics"):
            for key, value in train_result.metrics.items():
                if isinstance(value, (int, float)):
                    run.log_params({f"final/{key}": value})

        print(f"\nTraining completed in {training_time:.2f}s")
        print(f"Total steps: {trainer.state.global_step}")

    except KeyboardInterrupt:
        print("\nTraining interrupted by user")
        run.log_tags({"status": "interrupted"})
    except Exception as e:
        print(f"\nTraining failed: {e}")
        run.log_tags({"status": "failed", "error": str(e)})
        raise
    finally:
        run.finish()

    # Evaluation
    print("\nRunning evaluation...")
    correct = 0
    total = 0

    model.eval()
    with torch.no_grad():
        for example in eval_data[:5]:  # Quick eval on subset
            inputs = tokenizer(
                example["prompt"],
                return_tensors="pt",
                truncation=True,
                max_length=config.max_length,
            )
            inputs = {k: v.to(model.device) for k, v in inputs.items()}

            outputs = model.generate(
                **inputs,
                max_new_tokens=config.max_new_tokens,
                temperature=config.temperature,
                top_p=config.top_p,
                do_sample=True,
                pad_token_id=tokenizer.pad_token_id,
            )

            completion = tokenizer.decode(
                outputs[0][inputs["input_ids"].shape[1]:],
                skip_special_tokens=True,
            )

            reward = compute_reward([completion], example["answer"])[0]
            if reward > 0.5:
                correct += 1
            total += 1

            print(f"  Q: {example['prompt'][:50]}...")
            print(f"  A: {completion[:100]}...")
            print(f"  Reward: {reward}")
            print()

    eval_accuracy = correct / total if total > 0 else 0
    print(f"Evaluation accuracy: {eval_accuracy:.2%} ({correct}/{total})")

    print("\nRun completed!")
    print(f"View results at: http://localhost:3000/runs/{run.run_id}")


if __name__ == "__main__":
    main()
