from __future__ import annotations

from dataclasses import dataclass
import json
import mmap
import time


CONTROL_PROTOCOL_VERSION = 1


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


@dataclass(frozen=True)
class ControlRequest:
    protocol_version: int
    kind: str
    request_id: int
    slot_id: int
    slot_name: str
    byte_len: int
    tensor_dtype: str
    tensor_mel_bins: int
    tensor_frames: int


@dataclass(frozen=True)
class ControlResult:
    protocol_version: int
    kind: str
    request_id: int
    slot_id: int
    release_slot: bool
    ok: bool
    transcript_text: str
    error: str | None


def parse_control_request_line(line: str) -> ControlRequest:
    data = json.loads(line)
    request = ControlRequest(
        protocol_version=int(data["protocol_version"]),
        kind=str(data["kind"]),
        request_id=int(data["request_id"]),
        slot_id=int(data["slot_id"]),
        slot_name=str(data["slot_name"]),
        byte_len=int(data["byte_len"]),
        tensor_dtype=str(data["tensor_dtype"]),
        tensor_mel_bins=int(data["tensor_mel_bins"]),
        tensor_frames=int(data["tensor_frames"]),
    )
    validate_control_request(request)
    return request


def validate_control_request(request: ControlRequest) -> None:
    if request.protocol_version != CONTROL_PROTOCOL_VERSION:
        raise ValueError(
            f"unsupported control protocol version: {request.protocol_version}"
        )
    if request.kind != "transcribe-log-mel":
        raise ValueError(f"unsupported control request kind: {request.kind}")
    contract = TensorContract(
        dtype=request.tensor_dtype,
        mel_bins=request.tensor_mel_bins,
        frames=request.tensor_frames,
    )
    if request.byte_len != contract.byte_count:
        raise ValueError(
            f"expected {contract.byte_count} request bytes, got {request.byte_len}"
        )


def encode_control_result_line(result: ControlResult) -> str:
    return json.dumps(
        {
            "protocol_version": result.protocol_version,
            "kind": result.kind,
            "request_id": result.request_id,
            "slot_id": result.slot_id,
            "release_slot": result.release_slot,
            "ok": result.ok,
            "transcript_text": result.transcript_text,
            "error": result.error,
        },
        separators=(",", ":"),
    ) + "\n"


def validate_shared_memory_slot(request: ControlRequest) -> None:
    contract = TensorContract(
        dtype=request.tensor_dtype,
        mel_bins=request.tensor_mel_bins,
        frames=request.tensor_frames,
    )
    with mmap.mmap(
        -1,
        request.byte_len,
        tagname=request.slot_name,
        access=mmap.ACCESS_READ,
    ) as shared_memory:
        validate_tensor_payload(shared_memory[:], contract)


def debug_result_for_request(request: ControlRequest, text: str = "") -> ControlResult:
    return ControlResult(
        protocol_version=CONTROL_PROTOCOL_VERSION,
        kind="transcription-result",
        request_id=request.request_id,
        slot_id=request.slot_id,
        release_slot=True,
        ok=True,
        transcript_text=text,
        error=None,
    )


def run_debug_pipe_once(
    pipe_path: str,
    *,
    validate_slot: bool = False,
    retry_seconds: float = 5.0,
) -> None:
    deadline = time.monotonic() + retry_seconds
    while True:
        try:
            pipe = open(pipe_path, "r+b", buffering=0)
            break
        except FileNotFoundError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.01)

    with pipe:
        request_line = pipe.readline().decode("utf-8")
        request = parse_control_request_line(request_line)
        if validate_slot:
            validate_shared_memory_slot(request)
        pipe.write(encode_control_result_line(debug_result_for_request(request)).encode("utf-8"))
        pipe.flush()