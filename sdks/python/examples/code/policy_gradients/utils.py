# Utility Functions for Policy Gradient Training
#
# Original implementation by Zafir Stojanovski (@zafstojano)
# Source: https://github.com/zafstojano/policy-gradients
# License: Apache 2.0

import random
import math

from rich.console import Console
from rich.panel import Panel
from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    SpinnerColumn,
    TextColumn,
    TimeElapsedColumn,
)
from rich.table import Table


def print_step_header(console: Console, step: int, total: int) -> None:
    """Print a header for the current training step."""
    console.rule(f"[bold cyan]STEP {step + 1}/{total}[/bold cyan]", style="cyan")


def _format_value(value: float | None, spec: str) -> str:
    """Format numeric value for status lines."""
    if value is None:
        return "n/a"
    if not math.isfinite(value):
        return "n/a"
    return f"{value:{spec}}"


def print_step_summary(
    console: Console,
    step: int,
    total: int,
    step_time_s: float | None,
    loss: float | None,
    reward: float | None,
    throughput_toks_per_s: float | None,
    seq_len_toks_per_sample: float | None,
    async_level: float | None,
    max_off_policy_level: float | None,
) -> None:
    """Print one-line training summary in the expected SUCCESS format."""
    step_label = f"{step + 1}/{total}"
    summary = (
        "SUCCESS "
        f"Step {step_label} | "
        f"Time: {_format_value(step_time_s, '.2f')}s | "
        f"Loss: {_format_value(loss, '.4f')} | "
        f"Reward: {_format_value(reward, '.4f')} | "
        f"Throughput: {_format_value(throughput_toks_per_s, '.1f')} tokens/s | "
        f"Seq. Length: {_format_value(seq_len_toks_per_sample, '.1f')} tokens/sample | "
        f"Async Level: {_format_value(async_level, '.2f')} | "
        f"Max. Off-Policy Level: {_format_value(max_off_policy_level, '.4f')}"
    )
    # Write directly to the underlying stream to avoid Rich line wrapping in logs.
    console.file.write(summary + "\n")
    console.file.flush()


def progress_bar(console: Console, disable: bool = False) -> Progress:
    """Create a rich progress bar."""
    return Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        TextColumn("*"),
        TimeElapsedColumn(),
        console=console,
        disable=disable,
        transient=False,
    )


def print_model_info(console: Console, model) -> None:
    """Print model configuration information."""
    console.print(
        Panel(
            f"[bold magenta]Model:[/bold magenta] {model}\n"
            f"[dim]Parameters:[/dim] {sum(p.numel() for p in model.parameters()):,}\n"
            f"[dim]Device:[/dim] {model.device}",
            title="[bold magenta]Configuration[/bold magenta]",
            border_style="magenta",
        )
    )


def print_rollout_sample(console: Console, reward: float, rollout_completions: list) -> None:
    """Print a sample from the rollouts with the average reward."""
    sample_q, sample_a, sample_completion = random.choice(rollout_completions)
    console.print(
        Panel(
            f"[bold green]Average Reward:[/bold green] {reward:.4f}",
            title="[bold cyan]Rollout Results[/bold cyan]",
            border_style="cyan",
        )
    )
    sample_preview = sample_completion[:1000]
    if len(sample_completion) > 1000:
        sample_preview += "[dim]... (truncated)[/dim]"
    sample_table = Table(show_header=False, box=None, padding=(0, 1), show_edge=False)
    sample_table.add_column("Label", style="dim", width=12)
    sample_table.add_column("Content")
    sample_table.add_row("Question:", sample_q[:150] + ("..." if len(sample_q) > 150 else ""))
    sample_table.add_row("Oracle:", str(sample_a))
    sample_table.add_row("Completion:", sample_preview)
    console.print(Panel(sample_table, title="[bold cyan]Sample[/bold cyan]", border_style="dim"))
    console.print()
