from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any


DEFAULT_NUNIF_ROOT = Path(r"G:\Programming\Repos\nunif")
DEFAULT_MODEL_TYPE = "art"
DEFAULT_METHOD = "scale"
DEFAULT_NOISE_LEVEL = -1
DEFAULT_TILE_SIZE = 256
DEFAULT_BATCH_SIZE = 4
DEFAULT_NUNIF_HOME = Path(r"G:\Programming\Caches\NUNIF_HOME")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="teamy-waifu2x-reference")
    parser.add_argument(
        "--nunif-root",
        default=str(DEFAULT_NUNIF_ROOT),
        help="path to a local nunif checkout to import as the Python reference",
    )
    parser.add_argument(
        "--nunif-home",
        default=str(DEFAULT_NUNIF_HOME),
        help="NUNIF_HOME used for downloaded model/cache state",
    )
    parser.add_argument(
        "--download-models",
        action="store_true",
        help="download nunif waifu2x pretrained models before loading a model",
    )
    parser.add_argument(
        "--check-imports",
        action="store_true",
        help="import torch, torchvision, PIL, and local nunif, then print diagnostics as JSON",
    )
    parser.add_argument(
        "--cuda-check",
        action="store_true",
        help="print torch CUDA diagnostics as JSON",
    )
    parser.add_argument(
        "--model-report",
        action="store_true",
        help="load the selected nunif waifu2x model and print model metadata as JSON",
    )
    parser.add_argument(
        "--tensor-report",
        help="load an image and print nunif-compatible tensor summaries as JSON",
    )
    parser.add_argument(
        "--layer-report",
        action="store_true",
        help="run a deterministic input through the selected model and print major layer summaries as JSON",
    )
    parser.add_argument(
        "--layer-input-size",
        type=int,
        default=64,
        help="square deterministic input size for --layer-report",
    )
    parser.add_argument(
        "--layer-dump-npz",
        help="write layer tensors from --layer-report to a NumPy .npz archive",
    )
    parser.add_argument(
        "--write-fixture",
        help="write a deterministic tiny RGBA PNG fixture for tensor and alpha checks",
    )
    parser.add_argument(
        "--dump-npz",
        help="write inspectable tensors from --tensor-report to a NumPy .npz archive",
    )
    parser.add_argument(
        "--model-type",
        default=DEFAULT_MODEL_TYPE,
        help="nunif waifu2x model type, such as art, photo, or upconv_7/art",
    )
    parser.add_argument(
        "--method",
        default=DEFAULT_METHOD,
        help="nunif waifu2x method, default scale",
    )
    parser.add_argument(
        "--noise-level",
        type=int,
        default=DEFAULT_NOISE_LEVEL,
        help="nunif waifu2x noise level, default -1 for scale-only",
    )
    parser.add_argument(
        "--tile-size",
        type=int,
        default=DEFAULT_TILE_SIZE,
        help="nunif-compatible tile size",
    )
    parser.add_argument(
        "--batch-size",
        type=int,
        default=DEFAULT_BATCH_SIZE,
        help="nunif-compatible batch size",
    )
    parser.add_argument(
        "--device",
        default="cpu",
        choices=("cpu", "cuda"),
        help="reference execution device",
    )
    parser.add_argument(
        "--indent",
        type=int,
        default=2,
        help="JSON indentation; use 0 for compact output",
    )
    args = parser.parse_args(argv)

    try:
        configure_nunif_environment(Path(args.nunif_root), Path(args.nunif_home))
        if args.check_imports:
            print_json(check_imports(Path(args.nunif_root)), args.indent)
            return 0
        if args.cuda_check:
            print_json(cuda_report(), args.indent)
            return 0
        if args.model_report:
            print_json(model_report(args), args.indent)
            return 0
        if args.write_fixture:
            print_json(write_fixture(Path(args.write_fixture)), args.indent)
            return 0
        if args.tensor_report:
            print_json(tensor_report(args), args.indent)
            return 0
        if args.layer_report:
            print_json(layer_report(args), args.indent)
            return 0
    except Exception as error:  # noqa: BLE001 - CLI should report any reference failure.
        print(json.dumps({"ok": False, "error": str(error)}, indent=2), file=sys.stderr)
        return 1

    parser.print_help(sys.stderr)
    return 2


def configure_nunif_environment(nunif_root: Path, nunif_home: Path) -> None:
    root = nunif_root.resolve()
    if not root.is_dir():
        raise FileNotFoundError(f"nunif root does not exist: {root}")
    home = nunif_home.resolve()
    home.mkdir(parents=True, exist_ok=True)
    os.environ["NUNIF_HOME"] = str(home)
    root_text = str(root)
    if root_text not in sys.path:
        sys.path.insert(0, root_text)


def check_imports(nunif_root: Path) -> dict[str, Any]:
    import PIL
    import torch
    import torchvision
    import waifu2x.hub as waifu2x_hub

    return {
        "ok": True,
        "nunif_root": str(nunif_root.resolve()),
        "nunif_home": os.environ.get("NUNIF_HOME"),
        "python": sys.version,
        "torch": torch.__version__,
        "torchvision": torchvision.__version__,
        "pillow": PIL.__version__,
        "waifu2x_hub": str(Path(waifu2x_hub.__file__).resolve()),
        "cuda": cuda_report(),
    }


def cuda_report() -> dict[str, Any]:
    import torch

    available = torch.cuda.is_available()
    return {
        "available": available,
        "device_count": torch.cuda.device_count() if available else 0,
        "current_device": torch.cuda.current_device() if available else None,
        "device_name": torch.cuda.get_device_name(0) if available else None,
    }


def selected_model(args: argparse.Namespace):
    import torch
    from waifu2x.download_models import main as download_waifu2x_models
    from waifu2x.hub import Waifu2xImageModel

    if args.download_models:
        download_waifu2x_models()

    device_ids = [-1] if args.device == "cpu" else [0]
    model = Waifu2xImageModel(
        model_type=args.model_type,
        method=args.method,
        noise_level=args.noise_level,
        device_ids=device_ids,
        tile_size=args.tile_size,
        batch_size=args.batch_size,
        keep_alpha=True,
        amp=False,
    )
    if args.device == "cuda":
        if not torch.cuda.is_available():
            raise RuntimeError("CUDA was requested but torch.cuda.is_available() is false")
        model.cuda()
    else:
        model.cpu()
    return model


def model_report(args: argparse.Namespace) -> dict[str, Any]:
    model = selected_model(args)
    active = active_torch_model(model)
    return {
        "ok": True,
        "model_type": args.model_type,
        "method": model.method,
        "noise_level": model.noise_level,
        "device": str(model.device),
        "tile_size": model.tile_size,
        "batch_size": model.batch_size,
        "torch_model_class": type(active).__name__,
        "torch_model_name": getattr(active, "name", None),
        "i2i_scale": getattr(active, "i2i_scale", None),
        "i2i_offset": getattr(active, "i2i_offset", None),
        "i2i_blend_size": getattr(active, "i2i_blend_size", None),
        "i2i_default_tile_size": getattr(active, "i2i_default_tile_size", None),
        "i2i_default_batch_size": getattr(active, "i2i_default_batch_size", None),
        "state_dict_keys": list(active.state_dict().keys())[:32],
        "parameter_count": sum(parameter.numel() for parameter in active.parameters()),
    }


def active_torch_model(image_model: Any) -> Any:
    method = image_model.method
    noise_level = image_model.noise_level
    ctx = image_model.ctx
    if method == "scale":
        return ctx.scale_model
    if method == "scale4x":
        return ctx.scale4x_model
    if method == "noise":
        return ctx.noise_models[noise_level]
    if method == "noise_scale":
        return ctx.noise_scale_models[noise_level]
    if method == "noise_scale4x":
        return ctx.noise_scale4x_models[noise_level]
    raise ValueError(f"unsupported active method: {method}")


def tensor_report(args: argparse.Namespace) -> dict[str, Any]:
    import numpy as np
    import torch
    from nunif.utils import pil_io

    image_path = Path(args.tensor_report)
    image, meta = pil_io.load_image(str(image_path), color="rgb", keep_alpha=True)
    if image is None:
        raise RuntimeError(f"failed to load image: {image_path}")
    rgb, alpha = pil_io.to_tensor(image, return_alpha=True)
    blank_alpha = alpha is None or torch.equal(alpha, torch.ones_like(alpha))

    tensors: dict[str, Any] = {"rgb": rgb}
    if alpha is not None:
        tensors["alpha"] = alpha

    if args.dump_npz:
        arrays = {name: tensor.detach().cpu().numpy() for name, tensor in tensors.items()}
        np.savez(args.dump_npz, **arrays)

    return {
        "ok": True,
        "image_path": str(image_path),
        "metadata": safe_json(meta),
        "pil_mode": image.mode,
        "pil_size": list(image.size),
        "blank_alpha": bool(blank_alpha),
        "tensors": {name: tensor_summary(tensor) for name, tensor in tensors.items()},
        "dump_npz": args.dump_npz,
    }


def layer_report(args: argparse.Namespace) -> dict[str, Any]:
    import numpy as np
    import torch

    if args.layer_input_size <= 16:
        raise ValueError("--layer-input-size must be larger than 16 for swin_unet")

    image_model = selected_model(args)
    model = active_torch_model(image_model)
    model.eval()

    captures: dict[str, Any] = {}
    hooks = []
    for name, module in model.named_modules():
        if name in layer_capture_names(model):
            hooks.append(module.register_forward_hook(capture_layer(name, captures)))

    input_tensor = deterministic_model_input(args.layer_input_size, str(image_model.device))
    with torch.no_grad():
        output = model(input_tensor)
    for hook in hooks:
        hook.remove()

    tensors = {"input": input_tensor.detach().cpu(), "output": output.detach().cpu()}
    tensors.update({name: tensor.detach().cpu() for name, tensor in captures.items()})

    if args.layer_dump_npz:
        arrays = {npz_key(name): tensor.numpy() for name, tensor in tensors.items()}
        np.savez(args.layer_dump_npz, **arrays)

    return {
        "ok": True,
        "model_type": args.model_type,
        "method": image_model.method,
        "device": str(image_model.device),
        "input_size": args.layer_input_size,
        "layers": {name: tensor_summary(tensor) for name, tensor in tensors.items()},
        "dump_npz": args.layer_dump_npz,
    }


def layer_capture_names(model: Any) -> set[str]:
    names = {
        "unet.patch.0",
        "unet.patch.1",
        "unet.patch.2",
        "unet.patch.3",
        "unet.swin1",
        "unet.down1",
        "unet.swin2",
        "unet.down2",
        "unet.swin3",
        "unet.up2",
        "unet.swin4",
        "unet.up1",
        "unet.swin5",
        "unet.to_image",
    }
    return names.intersection(dict(model.named_modules()).keys())


def capture_layer(name: str, captures: dict[str, Any]):
    def hook(_module: Any, _inputs: Any, output: Any) -> None:
        captures[name] = output.detach().cpu()

    return hook


def deterministic_model_input(size: int, device: str):
    import torch

    values = torch.arange(3 * size * size, dtype=torch.float32, device=device)
    tensor = values.reshape(1, 3, size, size)
    return torch.remainder(tensor, 257.0) / 256.0


def npz_key(name: str) -> str:
    return name.replace(".", "_").replace("/", "_")


def write_fixture(path: Path) -> dict[str, Any]:
    from PIL import Image

    path.parent.mkdir(parents=True, exist_ok=True)
    image = Image.new("RGBA", (4, 4))
    pixels = [
        (0, 0, 0, 0),
        (255, 0, 0, 64),
        (0, 255, 0, 128),
        (0, 0, 255, 255),
        (255, 255, 255, 255),
        (64, 32, 16, 192),
        (16, 32, 64, 96),
        (128, 128, 0, 32),
        (0, 128, 128, 160),
        (128, 0, 128, 224),
        (32, 64, 96, 255),
        (96, 64, 32, 0),
        (255, 128, 0, 48),
        (0, 255, 128, 112),
        (128, 0, 255, 176),
        (255, 255, 0, 240),
    ]
    image.putdata(pixels)
    image.save(path)
    return {
        "ok": True,
        "fixture_path": str(path),
        "pil_mode": image.mode,
        "pil_size": list(image.size),
    }


def tensor_summary(tensor: Any) -> dict[str, Any]:
    data = tensor.detach().cpu().float()
    return {
        "shape": list(data.shape),
        "dtype": str(tensor.dtype),
        "min": float(data.min().item()),
        "max": float(data.max().item()),
        "mean": float(data.mean().item()),
        "sum": float(data.sum().item()),
    }


def safe_json(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): safe_json(item) for key, item in value.items()}
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def print_json(value: dict[str, Any], indent: int) -> None:
    if indent <= 0:
        print(json.dumps(value, separators=(",", ":")))
    else:
        print(json.dumps(value, indent=indent))


if __name__ == "__main__":
    raise SystemExit(main())