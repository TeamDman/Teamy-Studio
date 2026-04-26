from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class TensorContract:
    dtype: str
    mel_bins: int
    frames: int

    @property
    def value_count(self) -> int:
        return self.mel_bins * self.frames

    @property
    def byte_count(self) -> int:
        return self.value_count * 4


def default_tensor_contract() -> TensorContract:
    return TensorContract(dtype="f32-le", mel_bins=80, frames=3000)


def validate_tensor_payload(payload: bytes, contract: TensorContract | None = None) -> None:
    contract = contract or default_tensor_contract()
    if contract.dtype != "f32-le":
        raise ValueError(f"unsupported tensor dtype: {contract.dtype}")
    if len(payload) != contract.byte_count:
        raise ValueError(
            f"expected {contract.byte_count} tensor bytes, got {len(payload)}"
        )
    if len(payload) % 4 != 0:
        raise ValueError("tensor payload is not aligned to f32 values")