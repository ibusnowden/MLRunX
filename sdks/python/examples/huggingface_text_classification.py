"""HuggingFace Transformers text classification example with MLRunX SDK.

This example demonstrates how to integrate MLRunX with HuggingFace Transformers
for fine-tuning a text classification model.

Features demonstrated:
- Custom callback for logging training metrics
- Logging model hyperparameters
- Tracking training and evaluation metrics
- Integration with HuggingFace Trainer

Requirements:
    pip install transformers datasets torch

Usage:
    python huggingface_text_classification.py

Note: Works in offline mode if MLRunX server is not running.
"""

from __future__ import annotations

import importlib.util
import sys
from dataclasses import dataclass
from typing import Any

import mlrunx
from system_metrics import get_system_metrics, get_device_info

# Check for HuggingFace availability
try:
    if importlib.util.find_spec("torch") is None:
        raise ImportError
    from datasets import load_dataset
    from transformers import (
        AutoModelForSequenceClassification,
        AutoTokenizer,
        Trainer,
        TrainerCallback,
        TrainerControl,
        TrainerState,
        TrainingArguments,
    )
except ImportError:
    print("This example requires HuggingFace transformers and datasets.")
    print("Install with: pip install transformers datasets torch")
    sys.exit(1)


@dataclass
class MLRunXCallback(TrainerCallback):
    """HuggingFace Trainer callback for logging to MLRunX."""

    run: mlrunx.Run

    def on_log(
        self,
        args: TrainingArguments,
        state: TrainerState,
        control: TrainerControl,
        logs: dict[str, Any] | None = None,
        **kwargs: Any,
    ) -> None:
        """Log training metrics to MLRunX."""
        if logs is None:
            return

        # Get current step
        step = state.global_step

        # Prepare metrics for logging
        metrics = {}
        for key, value in logs.items():
            # Skip non-numeric values
            if not isinstance(value, (int, float)):
                continue

            # Prefix with train/ or eval/ based on key
            if key.startswith("eval_"):
                metrics[f"eval/{key[5:]}"] = value
            elif key in ("loss", "learning_rate", "grad_norm"):
                metrics[f"train/{key}"] = value
            else:
                metrics[key] = value

        # Add system metrics (GPU, CPU, memory)
        metrics.update(get_system_metrics())

        if metrics:
            self.run.log(metrics, step=step)

    def on_train_begin(
        self,
        args: TrainingArguments,
        state: TrainerState,
        control: TrainerControl,
        **kwargs: Any,
    ) -> None:
        """Log training arguments at start."""
        # Log key training arguments
        self.run.log_params({
            "trainer/num_train_epochs": args.num_train_epochs,
            "trainer/per_device_train_batch_size": args.per_device_train_batch_size,
            "trainer/per_device_eval_batch_size": args.per_device_eval_batch_size,
            "trainer/learning_rate": args.learning_rate,
            "trainer/weight_decay": args.weight_decay,
            "trainer/warmup_steps": args.warmup_steps,
            "trainer/max_steps": args.max_steps,
            "trainer/gradient_accumulation_steps": args.gradient_accumulation_steps,
            "trainer/fp16": args.fp16,
            "trainer/bf16": args.bf16,
        })

    def on_train_end(
        self,
        args: TrainingArguments,
        state: TrainerState,
        control: TrainerControl,
        **kwargs: Any,
    ) -> None:
        """Log final training state."""
        self.run.log_params({
            "final/global_step": state.global_step,
            "final/epoch": state.epoch,
            "final/best_metric": state.best_metric,
        })
        self.run.log_tags({"status": "completed"})


def tokenize_function(examples: dict, tokenizer: Any) -> dict:
    """Tokenize text examples."""
    return tokenizer(
        examples["text"],
        padding="max_length",
        truncation=True,
        max_length=128,
    )


def compute_metrics(eval_pred: tuple) -> dict[str, float]:
    """Compute evaluation metrics."""
    import numpy as np

    predictions, labels = eval_pred
    predictions = np.argmax(predictions, axis=1)
    accuracy = (predictions == labels).mean()
    return {"accuracy": accuracy}


def main() -> None:
    """Run text classification fine-tuning with MLRunX logging."""
    print("HuggingFace Text Classification with MLRunX")
    print("=" * 50)

    # Configuration
    model_name = "distilbert-base-uncased"
    dataset_name = "stanfordnlp/imdb"  # Full path for reliability
    num_train_samples = 1000  # Use subset for demo
    num_eval_samples = 500
    num_epochs = 1
    batch_size = 16
    learning_rate = 2e-5

    # Initialize MLRunX
    with mlrunx.init(
        project="huggingface-example",
        name="text-classification",
        tags={
            "framework": "transformers",
            "model": model_name,
            "dataset": dataset_name,
            "task": "text-classification",
        },
    ) as run:
        print(f"\nMLRunX Run ID: {run.run_id}")
        print(f"Offline mode: {run.is_offline}\n")

        # Log configuration
        run.log_params({
            "model_name": model_name,
            "dataset_name": dataset_name,
            "num_train_samples": num_train_samples,
            "num_eval_samples": num_eval_samples,
            "max_length": 128,
        })

        # Log device/system info
        run.log_params(get_device_info())

        # Load dataset
        print("Loading dataset...")
        dataset = load_dataset(dataset_name)

        # Use subset for faster demo
        train_dataset = dataset["train"].shuffle(seed=42).select(range(num_train_samples))
        eval_dataset = dataset["test"].shuffle(seed=42).select(range(num_eval_samples))

        run.log_params({
            "actual_train_samples": len(train_dataset),
            "actual_eval_samples": len(eval_dataset),
        })

        # Load tokenizer and model
        print(f"Loading model: {model_name}")
        tokenizer = AutoTokenizer.from_pretrained(model_name)
        model = AutoModelForSequenceClassification.from_pretrained(
            model_name,
            num_labels=2,
        )

        # Log model info
        total_params = sum(p.numel() for p in model.parameters())
        trainable_params = sum(p.numel() for p in model.parameters() if p.requires_grad)
        architecture = (
            model.config.architectures[0]
            if model.config.architectures
            else "unknown"
        )
        run.log_params({
            "model/total_params": total_params,
            "model/trainable_params": trainable_params,
            "model/architecture": architecture,
        })

        # Tokenize datasets
        print("Tokenizing datasets...")
        train_dataset = train_dataset.map(
            lambda x: tokenize_function(x, tokenizer),
            batched=True,
            remove_columns=["text"],
        )
        eval_dataset = eval_dataset.map(
            lambda x: tokenize_function(x, tokenizer),
            batched=True,
            remove_columns=["text"],
        )

        # Set format for PyTorch
        train_dataset.set_format("torch")
        eval_dataset.set_format("torch")

        # Training arguments
        training_args = TrainingArguments(
            output_dir="./results",
            num_train_epochs=num_epochs,
            per_device_train_batch_size=batch_size,
            per_device_eval_batch_size=batch_size,
            learning_rate=learning_rate,
            weight_decay=0.01,
            warmup_steps=100,
            logging_steps=10,
            eval_strategy="epoch",
            save_strategy="epoch",
            load_best_model_at_end=True,
            metric_for_best_model="accuracy",
            report_to="none",  # Disable default integrations
        )

        # Create MLRunX callback
        mlrunx_callback = MLRunXCallback(run=run)

        # Create Trainer
        trainer = Trainer(
            model=model,
            args=training_args,
            train_dataset=train_dataset,
            eval_dataset=eval_dataset,
            processing_class=tokenizer,  # Renamed from 'tokenizer' in newer versions
            compute_metrics=compute_metrics,
            callbacks=[mlrunx_callback],
        )

        # Train
        print("\nStarting training...")
        try:
            train_result = trainer.train()

            # Log final training metrics
            run.log_params({
                "final/train_loss": train_result.training_loss,
                "final/train_runtime": train_result.metrics.get("train_runtime", 0),
                "final/train_samples_per_second": train_result.metrics.get(
                    "train_samples_per_second", 0
                ),
            })

            # Evaluate
            print("\nEvaluating...")
            eval_results = trainer.evaluate()

            run.log_params({
                "final/eval_loss": eval_results.get("eval_loss", 0),
                "final/eval_accuracy": eval_results.get("eval_accuracy", 0),
            })

            print("\nTraining complete!")
            print(f"  Train loss: {train_result.training_loss:.4f}")
            print(f"  Eval accuracy: {eval_results.get('eval_accuracy', 0):.4f}")

        except KeyboardInterrupt:
            print("\nTraining interrupted by user")
            run.log_tags({"status": "interrupted"})

        # Run is automatically finished by context manager


if __name__ == "__main__":
    main()
