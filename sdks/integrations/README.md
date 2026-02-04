# MLRunX Integrations

Framework integrations for MLRunX.

## Supported Frameworks

- **PyTorch Lightning** - `MLRunXLogger`
- **HuggingFace Transformers** - `MLRunXCallback`
- **Optuna** - `MLRunXOptunaCallback`
- **Hydra** - `MLRunXHydraCallback`

## Installation

```bash
# Install with all integrations
pip install "mlrunx-integrations[all]"

# Or install specific integrations
pip install "mlrunx-integrations[lightning]"
pip install "mlrunx-integrations[transformers]"
pip install "mlrunx-integrations[optuna]"
pip install "mlrunx-integrations[hydra]"
```

## Usage

### PyTorch Lightning

```python
from mlrunx_integrations import MLRunXLogger
from pytorch_lightning import Trainer

trainer = Trainer(logger=MLRunXLogger(project="my-project"))
trainer.fit(model)
```

### HuggingFace Transformers

```python
from mlrunx_integrations import MLRunXCallback
from transformers import Trainer

trainer = Trainer(...)
trainer.add_callback(MLRunXCallback(project="my-project"))
trainer.train()
```

### Optuna

```python
from mlrunx_integrations import MLRunXOptunaCallback
import optuna

study = optuna.create_study()
study.optimize(
    objective,
    callbacks=[MLRunXOptunaCallback(project="my-project")]
)
```
