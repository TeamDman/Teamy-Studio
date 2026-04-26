from __future__ import annotations

from dataclasses import dataclass
import json
import mmap
import time
from typing import Callable


CONTROL_PROTOCOL_VERSION = 1
LOG_MEL_REQUEST_KIND = "transcribe-log-mel"
AUDIO_F32_REQUEST_KIND = "transcribe-audio-f32"
AUDIO_SAMPLE_RATE_HZ = 16_000


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
    if request.kind not in {LOG_MEL_REQUEST_KIND, AUDIO_F32_REQUEST_KIND}:
        raise ValueError(f"unsupported control request kind: {request.kind}")
    if request.tensor_dtype != "f32-le":
        raise ValueError(f"unsupported tensor dtype: {request.tensor_dtype}")
    expected_byte_count = request.tensor_mel_bins * request.tensor_frames * 4
    if request.byte_len != expected_byte_count:
        raise ValueError(
            f"expected {expected_byte_count} request bytes, got {request.byte_len}"
        )
    if request.kind == LOG_MEL_REQUEST_KIND:
        contract = default_tensor_contract()
        if request.tensor_mel_bins != contract.mel_bins:
            raise ValueError(f"expected {contract.mel_bins} mel bins")
        if request.tensor_frames != contract.frames:
            raise ValueError(f"expected {contract.frames} mel frames")
    elif request.tensor_mel_bins != 1:
        raise ValueError("audio f32 requests must use one sample lane")


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


def read_shared_memory_slot(request: ControlRequest) -> bytes:
    with mmap.mmap(
        -1,
        request.byte_len,
        tagname=request.slot_name,
        access=mmap.ACCESS_READ,
    ) as shared_memory:
        return shared_memory[:]


def validate_shared_memory_slot(request: ControlRequest) -> None:
    payload = read_shared_memory_slot(request)
    if request.kind == AUDIO_F32_REQUEST_KIND:
        validate_tensor_payload(
            payload,
            TensorContract(
                dtype=request.tensor_dtype,
                mel_bins=request.tensor_mel_bins,
                frames=request.tensor_frames,
            ),
        )
        return
    contract = TensorContract(
        dtype=request.tensor_dtype,
        mel_bins=request.tensor_mel_bins,
        frames=request.tensor_frames,
    )
    validate_tensor_payload(payload, contract)


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


def error_result_for_request(request: ControlRequest, error: str) -> ControlResult:
    return ControlResult(
        protocol_version=CONTROL_PROTOCOL_VERSION,
        kind="transcription-result",
        request_id=request.request_id,
        slot_id=request.slot_id,
        release_slot=True,
        ok=False,
        transcript_text="",
        error=error,
    )


def transcript_result_for_request(request: ControlRequest, text: str) -> ControlResult:
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


def transcribe_shared_memory_audio(
    request: ControlRequest,
    *,
    model_name: str,
    backend: str = "auto",
) -> ControlResult:
    if request.kind != AUDIO_F32_REQUEST_KIND:
        raise ValueError(f"real transcription requires {AUDIO_F32_REQUEST_KIND}")
    payload = read_shared_memory_slot(request)
    validate_tensor_payload(
        payload,
        TensorContract(
            dtype=request.tensor_dtype,
            mel_bins=request.tensor_mel_bins,
            frames=request.tensor_frames,
        ),
    )
    try:
        transcript = _transcribe_f32_payload(payload, model_name=model_name, backend=backend)
    except Exception as exc:  # noqa: BLE001 - surfaced over the control protocol.
        return error_result_for_request(request, str(exc))
    return transcript_result_for_request(request, transcript)


def _transcribe_f32_payload(payload: bytes, *, model_name: str, backend: str) -> str:
    try:
        import numpy as np
    except ImportError as exc:
        raise RuntimeError(
            "numpy is required for transcription; run this daemon with `uv run --extra transcription`"
        ) from exc
    audio = np.frombuffer(payload, dtype="<f4").astype("float32", copy=True)
    if audio.size == 0:
        return ""
    audio = np.nan_to_num(audio, nan=0.0, posinf=0.0, neginf=0.0)
    audio = np.clip(audio, -1.0, 1.0)
    backends: list[Callable[[object, str], str]] = []
    if backend in {"auto", "whisperx"}:
        backends.append(_transcribe_with_whisperx)
    if backend in {"auto", "whisper"}:
        backends.append(_transcribe_with_openai_whisper)
    errors: list[str] = []
    for transcriber in backends:
        try:
            return transcriber(audio, model_name).strip()
        except ImportError as exc:
            errors.append(str(exc))
        except Exception as exc:  # noqa: BLE001 - try next backend or surface context.
            errors.append(str(exc))
            if backend != "auto":
                break
    raise RuntimeError("; ".join(errors) or "no transcription backend is available")


def _torch_device() -> str:
    try:
        import torch
    except ImportError:
        return "cpu"
    return "cuda" if torch.cuda.is_available() else "cpu"


def cuda_check_report() -> dict[str, object]:
    try:
        import torch
    except ImportError as exc:
        return {
            "ok": False,
            "torch_imported": False,
            "cuda_available": False,
            "error": str(exc),
        }
    cuda_available = bool(torch.cuda.is_available())
    device_count = int(torch.cuda.device_count()) if cuda_available else 0
    devices = []
    for index in range(device_count):
        devices.append(
            {
                "index": index,
                "name": torch.cuda.get_device_name(index),
                "capability": ".".join(map(str, torch.cuda.get_device_capability(index))),
            }
        )
    return {
        "ok": cuda_available,
        "torch_imported": True,
        "torch_version": getattr(torch, "__version__", "unknown"),
        "cuda_available": cuda_available,
        "cuda_version": getattr(torch.version, "cuda", None),
        "device_count": device_count,
        "devices": devices,
        "error": None if cuda_available else "torch.cuda.is_available() returned False",
    }


def _transcribe_with_whisperx(audio: object, model_name: str) -> str:
    try:
        import whisperx
    except ImportError as exc:
        raise ImportError("whisperx is not installed") from exc
    device = _torch_device()
    compute_type = "float16" if device == "cuda" else "int8"
    model = whisperx.load_model(model_name, device=device, compute_type=compute_type)
    result = model.transcribe(audio, batch_size=8)
    segments = result.get("segments") or []
    if segments:
        return " ".join(str(segment.get("text", "")).strip() for segment in segments).strip()
    return str(result.get("text", "")).strip()


def _transcribe_with_openai_whisper(audio: object, model_name: str) -> str:
    try:
        import whisper
    except ImportError as exc:
        raise ImportError(
            "openai-whisper is not installed; run this daemon with `uv run --extra transcription`"
        ) from exc
    device = _torch_device()
    model = whisper.load_model(model_name, device=device)
    result = model.transcribe(audio, fp16=device == "cuda")
    return str(result.get("text", "")).strip()


def run_debug_pipe_once(
    pipe_path: str,
    *,
    validate_slot: bool = False,
    transcript_text: str = "",
    transcribe_slot: bool = False,
    model_name: str = "small.en",
    backend: str = "auto",
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
        if transcribe_slot:
            result = transcribe_shared_memory_audio(
                request,
                model_name=model_name,
                backend=backend,
            )
        else:
            result = debug_result_for_request(request, transcript_text)
        pipe.write(
            encode_control_result_line(result).encode("utf-8")
        )
        pipe.flush()