"""PyTorch MNIST training example with MLRunX SDK.

This example demonstrates how to integrate MLRunX with a real PyTorch
training loop for MNIST classification.

Features demonstrated:
- Logging hyperparameters at the start
- Logging training metrics per step
- Logging validation metrics per epoch
- Using context manager for automatic cleanup
- Graceful handling of interrupts

Requirements:
    pip install torch torchvision

Usage:
    python pytorch_mnist.py

Note: Works in offline mode if MLRunX server is not running.
"""

from __future__ import annotations

import argparse
import sys

import mlrunx
from system_metrics import get_system_metrics, get_device_info

# Check for PyTorch availability
try:
    import torch
    import torch.nn as nn
    import torch.nn.functional as F
    import torch.optim as optim
    from torch.utils.data import DataLoader
    from torchvision import datasets, transforms
except ImportError:
    print("This example requires PyTorch and torchvision.")
    print("Install with: pip install torch torchvision")
    sys.exit(1)


class SimpleCNN(nn.Module):
    """Simple CNN for MNIST classification."""

    def __init__(self) -> None:
        super().__init__()
        self.conv1 = nn.Conv2d(1, 32, 3, 1)
        self.conv2 = nn.Conv2d(32, 64, 3, 1)
        self.dropout1 = nn.Dropout(0.25)
        self.dropout2 = nn.Dropout(0.5)
        self.fc1 = nn.Linear(9216, 128)
        self.fc2 = nn.Linear(128, 10)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self.conv1(x)
        x = F.relu(x)
        x = self.conv2(x)
        x = F.relu(x)
        x = F.max_pool2d(x, 2)
        x = self.dropout1(x)
        x = torch.flatten(x, 1)
        x = self.fc1(x)
        x = F.relu(x)
        x = self.dropout2(x)
        x = self.fc2(x)
        return F.log_softmax(x, dim=1)


def train_epoch(
    model: nn.Module,
    device: torch.device,
    train_loader: DataLoader,
    optimizer: optim.Optimizer,
    epoch: int,
    run: mlrunx.Run,
    log_interval: int = 100,
) -> float:
    """Train for one epoch and log metrics to MLRunX."""
    model.train()
    total_loss = 0.0
    correct = 0
    total = 0
    global_step = epoch * len(train_loader)

    for batch_idx, (data, target) in enumerate(train_loader):
        data, target = data.to(device), target.to(device)

        optimizer.zero_grad()
        output = model(data)
        loss = F.nll_loss(output, target)
        loss.backward()
        optimizer.step()

        # Track metrics
        total_loss += loss.item()
        pred = output.argmax(dim=1, keepdim=True)
        correct += pred.eq(target.view_as(pred)).sum().item()
        total += len(data)

        # Log to MLRunX (non-blocking)
        step = global_step + batch_idx
        metrics = {
            "train/loss": loss.item(),
            "train/accuracy": correct / total,
        }
        # Add system metrics (GPU, CPU, memory)
        metrics.update(get_system_metrics())
        run.log(metrics, step=step)

        # Print progress
        if batch_idx % log_interval == 0:
            print(
                f"  Epoch {epoch} [{batch_idx * len(data)}/{len(train_loader.dataset)}] "
                f"Loss: {loss.item():.6f}"
            )

    avg_loss = total_loss / len(train_loader)
    return avg_loss


def validate(
    model: nn.Module,
    device: torch.device,
    val_loader: DataLoader,
    epoch: int,
    run: mlrunx.Run,
) -> tuple[float, float]:
    """Validate and log metrics to MLRunX."""
    model.eval()
    val_loss = 0.0
    correct = 0

    with torch.no_grad():
        for data, target in val_loader:
            data, target = data.to(device), target.to(device)
            output = model(data)
            val_loss += F.nll_loss(output, target, reduction="sum").item()
            pred = output.argmax(dim=1, keepdim=True)
            correct += pred.eq(target.view_as(pred)).sum().item()

    val_loss /= len(val_loader.dataset)
    accuracy = correct / len(val_loader.dataset)

    # Log validation metrics at epoch level
    run.log(
        {
            "val/loss": val_loss,
            "val/accuracy": accuracy,
        },
        step=epoch,
    )

    print(f"  Validation: Loss: {val_loss:.4f}, Accuracy: {accuracy:.4f}")
    return val_loss, accuracy


def main() -> None:
    """Run MNIST training with MLRunX logging."""
    parser = argparse.ArgumentParser(description="PyTorch MNIST with MLRunX")
    parser.add_argument("--epochs", type=int, default=3, help="number of epochs")
    parser.add_argument("--batch-size", type=int, default=64, help="batch size")
    parser.add_argument("--lr", type=float, default=1.0, help="learning rate")
    parser.add_argument("--gamma", type=float, default=0.7, help="LR decay factor")
    parser.add_argument("--no-cuda", action="store_true", help="disable CUDA")
    parser.add_argument("--seed", type=int, default=42, help="random seed")
    args = parser.parse_args()

    # Setup device
    use_cuda = not args.no_cuda and torch.cuda.is_available()
    device = torch.device("cuda" if use_cuda else "cpu")
    print(f"Using device: {device}")

    torch.manual_seed(args.seed)

    # Data loaders
    transform = transforms.Compose([
        transforms.ToTensor(),
        transforms.Normalize((0.1307,), (0.3081,)),
    ])

    train_dataset = datasets.MNIST(
        "./data", train=True, download=True, transform=transform
    )
    val_dataset = datasets.MNIST("./data", train=False, transform=transform)

    train_loader = DataLoader(
        train_dataset,
        batch_size=args.batch_size,
        shuffle=True,
        num_workers=2,
    )
    val_loader = DataLoader(
        val_dataset,
        batch_size=1000,
        shuffle=False,
        num_workers=2,
    )

    # Initialize MLRunX run using context manager
    with mlrunx.init(
        project="mnist-example",
        name="pytorch-cnn",
        tags={"framework": "pytorch", "model": "cnn", "dataset": "mnist"},
    ) as run:
        print(f"\nMLRunX Run ID: {run.run_id}")
        print(f"Offline mode: {run.is_offline}\n")

        # Log hyperparameters
        run.log_params({
            "epochs": args.epochs,
            "batch_size": args.batch_size,
            "learning_rate": args.lr,
            "lr_gamma": args.gamma,
            "optimizer": "Adadelta",
            "scheduler": "StepLR",
            "device": str(device),
            "seed": args.seed,
        })

        # Log device/system info
        run.log_params(get_device_info())

        # Model setup
        model = SimpleCNN().to(device)
        optimizer = optim.Adadelta(model.parameters(), lr=args.lr)
        scheduler = optim.lr_scheduler.StepLR(optimizer, step_size=1, gamma=args.gamma)

        # Log model info
        total_params = sum(p.numel() for p in model.parameters())
        trainable_params = sum(p.numel() for p in model.parameters() if p.requires_grad)
        run.log_params({
            "model/total_params": total_params,
            "model/trainable_params": trainable_params,
        })

        # Training loop
        best_val_acc = 0.0
        try:
            for epoch in range(args.epochs):
                print(f"\nEpoch {epoch + 1}/{args.epochs}")
                print("-" * 40)

                train_epoch(model, device, train_loader, optimizer, epoch, run)
                val_loss, val_acc = validate(model, device, val_loader, epoch, run)

                scheduler.step()

                # Log learning rate
                current_lr = scheduler.get_last_lr()[0]
                run.log({"learning_rate": current_lr}, step=epoch)

                # Track best accuracy
                if val_acc > best_val_acc:
                    best_val_acc = val_acc
                    run.log_tags({"best_epoch": str(epoch)})

        except KeyboardInterrupt:
            print("\nTraining interrupted by user")
            run.log_tags({"status": "interrupted"})

        # Log final metrics
        run.log_params({
            "final/val_loss": val_loss,
            "final/val_accuracy": val_acc,
            "final/best_accuracy": best_val_acc,
        })

        print(f"\nTraining complete! Best validation accuracy: {best_val_acc:.4f}")
        # Run is automatically finished by context manager


if __name__ == "__main__":
    main()
