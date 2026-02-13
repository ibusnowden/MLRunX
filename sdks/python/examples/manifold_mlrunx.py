"""
Manifold Optimization Example with MLRunX Integration

This example trains a 3-layer MLP on CIFAR-10 using manifold optimization
techniques (Manifold Muon, Hyperspherical Descent) and logs comprehensive
metrics to MLRunX for visualization.

Metrics logged:
- Gradient Norm (global)
- Gradient Norm (per layer: fc1, fc2, fc3)
- Activation statistics (mean, std per layer)
- Cross-Entropy Loss (log-scale)
- Next Token Entropy (for classification)
- Weight norms and singular values

Usage:
    python manifold_mlrunx.py --update manifold_muon --epochs 10
    python manifold_mlrunx.py --update hyperspherical_descent --epochs 10
    python manifold_mlrunx.py --update adam --epochs 10 --wd 0.01
"""

import argparse
import math
import sys
import os

import torch
import torch.nn as nn
import torch.nn.functional as F
import torchvision
import torchvision.transforms as transforms
from torch.optim import AdamW
from torch.utils.data import DataLoader

# Add manifold_src to path for imports
sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'manifold_src'))

# Import manifold optimization methods
from manifold_muon import manifold_muon
from hyperspherical import hyperspherical_descent

# Import MLRunX
import mlrunx
from system_metrics import get_system_metrics, get_device_info


def get_device():
    """Get the best available device."""
    if torch.cuda.is_available():
        return torch.device('cuda')
    elif torch.backends.mps.is_available():
        return torch.device('mps')
    return torch.device('cpu')


def get_data_loaders(batch_size=512):
    """Create CIFAR-10 data loaders."""
    transform = transforms.Compose([
        transforms.ToTensor(),
        transforms.Normalize(
            (0.49139968, 0.48215827, 0.44653124),
            (0.24703233, 0.24348505, 0.26158768)
        )
    ])

    train_dataset = torchvision.datasets.CIFAR10(
        root="./data", train=True, transform=transform, download=True
    )
    test_dataset = torchvision.datasets.CIFAR10(
        root="./data", train=False, transform=transform, download=True
    )

    train_loader = DataLoader(
        dataset=train_dataset, batch_size=batch_size, shuffle=True, num_workers=0
    )
    test_loader = DataLoader(
        dataset=test_dataset, batch_size=batch_size, shuffle=False, num_workers=0
    )

    return train_loader, test_loader


class MLP(nn.Module):
    """3-layer MLP for CIFAR-10 classification."""

    def __init__(self):
        super(MLP, self).__init__()
        self.fc1 = nn.Linear(32 * 32 * 3, 128, bias=False)
        self.fc2 = nn.Linear(128, 64, bias=False)
        self.fc3 = nn.Linear(64, 10, bias=False)

        # Store activations for logging
        self.activations = {}

    def forward(self, x):
        x = x.view(-1, 32 * 32 * 3)

        # Layer 1
        x = self.fc1(x)
        self.activations['fc1_pre'] = x.detach()
        x = torch.relu(x)
        self.activations['fc1_post'] = x.detach()

        # Layer 2
        x = self.fc2(x)
        self.activations['fc2_pre'] = x.detach()
        x = torch.relu(x)
        self.activations['fc2_post'] = x.detach()

        # Layer 3 (output)
        x = self.fc3(x)
        self.activations['fc3_logits'] = x.detach()

        return x


def compute_gradient_norms(model):
    """Compute gradient norms globally and per layer."""
    grad_norms = {}
    all_grads = []

    for name, param in model.named_parameters():
        if param.grad is not None:
            grad_norm = param.grad.norm().item()
            grad_norms[f'grad_norm/{name}'] = grad_norm
            all_grads.append(param.grad.flatten())

    # Global gradient norm
    if all_grads:
        global_grad = torch.cat(all_grads)
        grad_norms['grad_norm/global'] = global_grad.norm().item()
        grad_norms['grad_norm/global_mean'] = global_grad.abs().mean().item()
        grad_norms['grad_norm/global_max'] = global_grad.abs().max().item()

    return grad_norms


def compute_activation_stats(model):
    """Compute activation statistics per layer."""
    stats = {}

    for name, activation in model.activations.items():
        stats[f'activation/{name}_mean'] = activation.mean().item()
        stats[f'activation/{name}_std'] = activation.std().item()
        stats[f'activation/{name}_max'] = activation.abs().max().item()
        # Sparsity (fraction of zeros after ReLU)
        if 'post' in name:
            sparsity = (activation == 0).float().mean().item()
            stats[f'activation/{name}_sparsity'] = sparsity

    return stats


def compute_weight_stats(model):
    """Compute weight statistics and singular values."""
    stats = {}

    for name, param in model.named_parameters():
        stats[f'weight/{name}_norm'] = param.norm().item()
        stats[f'weight/{name}_mean'] = param.mean().item()
        stats[f'weight/{name}_std'] = param.std().item()

        # Singular values for 2D weights
        if param.ndim == 2:
            try:
                s = torch.linalg.svdvals(param.detach().float())
                stats[f'weight/{name}_sv_max'] = s[0].item()
                stats[f'weight/{name}_sv_min'] = s[-1].item()
                stats[f'weight/{name}_sv_ratio'] = (s[0] / (s[-1] + 1e-8)).item()
            except:
                pass

    return stats


def compute_entropy(logits):
    """Compute entropy of the prediction distribution."""
    probs = F.softmax(logits, dim=-1)
    log_probs = F.log_softmax(logits, dim=-1)
    entropy = -(probs * log_probs).sum(dim=-1).mean()
    return entropy.item()


def train(args, run):
    """Main training loop with MLRunX logging."""
    device = get_device()
    print(f"Using device: {device}")

    train_loader, test_loader = get_data_loaders(batch_size=args.batch_size)
    model = MLP().to(device)
    criterion = nn.CrossEntropyLoss()

    # Setup optimizer based on update rule
    update_rules = {
        "manifold_muon": manifold_muon,
        "hyperspherical_descent": hyperspherical_descent,
        "adam": AdamW
    }
    update = update_rules[args.update]

    if update == AdamW:
        optimizer = AdamW(model.parameters(), lr=args.lr, weight_decay=args.wd)
    else:
        optimizer = None
        # Project weights to manifold initially
        for p in model.parameters():
            p.data = update(p.data, torch.zeros_like(p.data), eta=0)

    total_steps = args.epochs * len(train_loader)
    global_step = 0

    # Log hyperparameters as params (not metrics)
    run.log_params({
        'learning_rate': args.lr,
        'batch_size': args.batch_size,
        'epochs': args.epochs,
        'update_rule': args.update,
        'weight_decay': args.wd,
        'device': str(device),
    })

    # Log device/system info
    run.log_params(get_device_info())

    for epoch in range(args.epochs):
        model.train()
        epoch_loss = 0.0
        epoch_correct = 0
        epoch_total = 0

        for batch_idx, (images, labels) in enumerate(train_loader):
            images = images.to(device)
            labels = labels.to(device)

            # Forward pass
            outputs = model(images)
            loss = criterion(outputs, labels)

            # Compute accuracy
            _, predicted = torch.max(outputs.data, 1)
            epoch_total += labels.size(0)
            epoch_correct += (predicted == labels).sum().item()

            # Backward pass
            model.zero_grad()
            loss.backward()

            # Compute gradient norms before update
            grad_norms = compute_gradient_norms(model)

            # Learning rate schedule (linear decay)
            lr = args.lr * (1 - global_step / total_steps)

            # Apply update
            with torch.no_grad():
                if optimizer is None:
                    # Manifold update
                    for p in model.parameters():
                        p.data = update(p, p.grad, eta=lr)
                else:
                    # Adam update
                    for param_group in optimizer.param_groups:
                        param_group["lr"] = lr
                    optimizer.step()

            epoch_loss += loss.item()

            # Log metrics every N steps
            if global_step % args.log_interval == 0:
                # Loss metrics (log scale friendly)
                loss_val = loss.item()
                metrics = {
                    'loss/cross_entropy': loss_val,
                    'loss/cross_entropy_log': math.log(loss_val + 1e-8),
                    'loss/running_avg': epoch_loss / (batch_idx + 1),
                }

                # Entropy metrics
                entropy = compute_entropy(model.activations['fc3_logits'])
                metrics['entropy/prediction'] = entropy
                metrics['entropy/prediction_log'] = math.log(entropy + 1e-8)

                # Gradient norms
                metrics.update(grad_norms)

                # Activation stats
                activation_stats = compute_activation_stats(model)
                metrics.update(activation_stats)

                # Weight stats (less frequently)
                if global_step % (args.log_interval * 10) == 0:
                    weight_stats = compute_weight_stats(model)
                    metrics.update(weight_stats)

                # Learning rate
                metrics['lr'] = lr

                # System metrics (GPU, CPU, memory)
                metrics.update(get_system_metrics())

                # Log to MLRunX
                run.log(metrics, step=global_step)

            global_step += 1

        # End of epoch metrics
        epoch_acc = 100 * epoch_correct / epoch_total
        avg_loss = epoch_loss / len(train_loader)

        print(f"Epoch [{epoch+1}/{args.epochs}] - Loss: {avg_loss:.4f}, Train Acc: {epoch_acc:.2f}%")

        # Evaluate on test set
        test_acc, test_loss = evaluate(model, test_loader, criterion, device)
        print(f"  Test Loss: {test_loss:.4f}, Test Acc: {test_acc:.2f}%")

        # Log epoch metrics
        run.log({
            'epoch/train_loss': avg_loss,
            'epoch/train_acc': epoch_acc,
            'epoch/test_loss': test_loss,
            'epoch/test_acc': test_acc,
            'epoch': epoch + 1,
        }, step=global_step)

    return model


def evaluate(model, test_loader, criterion, device):
    """Evaluate model on test set."""
    model.eval()
    test_loss = 0.0
    correct = 0
    total = 0

    with torch.no_grad():
        for images, labels in test_loader:
            images = images.to(device)
            labels = labels.to(device)
            outputs = model(images)
            loss = criterion(outputs, labels)
            test_loss += loss.item()
            _, predicted = torch.max(outputs.data, 1)
            total += labels.size(0)
            correct += (predicted == labels).sum().item()

    return 100 * correct / total, test_loss / len(test_loader)


def main():
    parser = argparse.ArgumentParser(description="Manifold Optimization with MLRunX")
    parser.add_argument("--epochs", type=int, default=5, help="Number of epochs")
    parser.add_argument("--lr", type=float, default=0.1, help="Initial learning rate")
    parser.add_argument("--batch_size", type=int, default=512, help="Batch size")
    parser.add_argument("--update", type=str, default="manifold_muon",
                        choices=["manifold_muon", "hyperspherical_descent", "adam"],
                        help="Update rule to use")
    parser.add_argument("--wd", type=float, default=0.0, help="Weight decay (Adam only)")
    parser.add_argument("--seed", type=int, default=42, help="Random seed")
    parser.add_argument("--log_interval", type=int, default=10, help="Steps between logging")
    args = parser.parse_args()

    # Set seeds for reproducibility
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)
        torch.backends.cudnn.deterministic = True
        torch.backends.cudnn.benchmark = False

    # Initialize MLRunX
    run = mlrunx.init(
        project_id=os.getenv("MLRUNX_PROJECT_ID", "project-uuid"),
        name=f"{args.update}-lr{args.lr}-bs{args.batch_size}",
        tags={
            "update_rule": args.update,
            "model": "3-layer-mlp",
            "dataset": "cifar10",
        },
    )

    print(f"\n{'='*60}")
    print(f"Manifold Optimization Experiment")
    print(f"{'='*60}")
    print(f"Update Rule: {args.update}")
    print(f"Epochs: {args.epochs}")
    print(f"Learning Rate: {args.lr}")
    print(f"Batch Size: {args.batch_size}")
    print(f"Run ID: {run.run_id}")
    print(f"Offline Mode: {run.is_offline}")
    print(f"{'='*60}\n")

    try:
        model = train(args, run)
        print("\nTraining completed successfully!")
    except KeyboardInterrupt:
        print("\nTraining interrupted by user")
    except Exception as e:
        print(f"\nTraining failed: {e}")
        raise
    finally:
        run.finish()
        print(f"\nRun finished. ID: {run.run_id}")


if __name__ == "__main__":
    main()
