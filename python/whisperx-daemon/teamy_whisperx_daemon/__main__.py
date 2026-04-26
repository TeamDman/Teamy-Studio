from __future__ import annotations

import argparse
import json
import sys

from .protocol import (
    debug_result_for_request,
    default_tensor_contract,
    encode_control_result_line,
    parse_control_request_line,
    run_debug_pipe_once,
    transcribe_shared_memory_audio,
    validate_shared_memory_slot,
    validate_tensor_payload,
)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="teamy-whisperx-daemon")
    parser.add_argument(
        "--print-contract",
        action="store_true",
        help="print the Rust/Python tensor contract as JSON",
    )
    parser.add_argument(
        "--validate-zero-payload",
        action="store_true",
        help="validate an all-zero payload with the default tensor shape",
    )
    parser.add_argument(
        "--validate-control-request",
        help="validate one Rust JSONL control request and print a debug result",
    )
    parser.add_argument(
        "--validate-shared-memory-slot",
        action="store_true",
        help="map and validate the shared-memory slot named by --validate-control-request",
    )
    parser.add_argument(
        "--connect-pipe-once",
        help="connect to a Rust-owned named pipe, handle one debug request, then exit",
    )
    parser.add_argument(
        "--debug-transcript-text",
        default="",
        help="transcript text to return from --connect-pipe-once",
    )
    parser.add_argument(
        "--transcribe-shared-memory-slot",
        action="store_true",
        help="run real transcription for a transcribe-audio-f32 request",
    )
    parser.add_argument(
        "--model",
        default="small.en",
        help="Whisper/WhisperX model name for --transcribe-shared-memory-slot",
    )
    parser.add_argument(
        "--backend",
        default="auto",
        choices=("auto", "whisperx", "whisper"),
        help="transcription backend preference",
    )
    args = parser.parse_args(argv)

    contract = default_tensor_contract()
    if args.connect_pipe_once:
        run_debug_pipe_once(
            args.connect_pipe_once,
            validate_slot=args.validate_shared_memory_slot,
            transcript_text=args.debug_transcript_text,
            transcribe_slot=args.transcribe_shared_memory_slot,
            model_name=args.model,
            backend=args.backend,
        )
        return 0

    if args.validate_control_request:
        request = parse_control_request_line(args.validate_control_request)
        if args.validate_shared_memory_slot:
            validate_shared_memory_slot(request)
        if args.transcribe_shared_memory_slot:
            print(
                encode_control_result_line(
                    transcribe_shared_memory_audio(
                        request,
                        model_name=args.model,
                        backend=args.backend,
                    )
                ),
                end="",
            )
            return 0
        print(encode_control_result_line(debug_result_for_request(request)), end="")
        return 0

    if args.validate_zero_payload:
        validate_tensor_payload(bytes(contract.byte_count), contract)

    if args.print_contract or args.validate_zero_payload:
        print(
            json.dumps(
                {
                    "dtype": contract.dtype,
                    "mel_bins": contract.mel_bins,
                    "frames": contract.frames,
                    "values": contract.value_count,
                    "bytes": contract.byte_count,
                },
                indent=2,
            )
        )
        return 0

    parser.print_help(sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())