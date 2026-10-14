"""OSC 1.0 UDP transport; no assumed MOTU addresses or automatic writes.

Only explicit send() calls transmit. Receiving data is not proof that a
control change was accepted by CueMix. Scheduled bundles are decoded, not
executed. Use the device's manual for addressing, scaling and handshake.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import ipaddress
import math
import socket
import struct


@dataclass(frozen=True)
class Message:
    address: str
    arguments: tuple[int | float | str | bool, ...] = ()


@dataclass(frozen=True)
class Bundle:
    timetag: int
    elements: tuple[Message | Bundle, ...]


def _string(value: str) -> bytes:
    if "\0" in value or not value.isascii():
        raise ValueError("OSC strings must be ASCII without embedded NULs")
    raw = value.encode("ascii") + b"\0"
    return raw + b"\0" * (-len(raw) % 4)


def encode(message: Message) -> bytes:
    if not message.address.startswith("/"):
        raise ValueError("OSC addresses must start with /")
    tags = ","
    values = bytearray()
    for value in message.arguments:
        if isinstance(value, bool):
            tags += "T" if value else "F"
        elif isinstance(value, int):
            if not -(2**31) <= value < 2**31:
                raise ValueError("OSC integer must fit signed int32")
            tags += "i"
            values.extend(struct.pack(">i", value))
        elif isinstance(value, float):
            if not math.isfinite(value) or abs(value) > 3.4028234663852886e38:
                raise ValueError("control float must be finite float32")
            tags += "f"
            values.extend(struct.pack(">f", value))
        elif isinstance(value, str):
            tags += "s"
            values.extend(_string(value))
        else:
            raise ValueError("unsupported OSC argument type")
    packet = _string(message.address) + _string(tags) + values
    if len(packet) > 65507:
        raise ValueError("packet exceeds IPv4 UDP payload limit")
    return packet


def decode(packet: bytes, *, _depth: int = 0) -> Message | Bundle:
    if not packet or len(packet) % 4 or _depth > 8:
        raise ValueError("invalid OSC size or bundle nesting")
    offset = 0

    def read_string() -> str:
        nonlocal offset
        end = packet.find(b"\0", offset)
        if end < 0:
            raise ValueError("unterminated OSC string")
        padded = (end + 4) & ~3
        if padded > len(packet) or any(packet[end:padded]):
            raise ValueError("invalid OSC string padding")
        try:
            value = packet[offset:end].decode("ascii")
        except UnicodeDecodeError as error:
            raise ValueError("non-ASCII OSC string") from error
        offset = padded
        return value

    def read_number(fmt: str, size: int):
        nonlocal offset
        if offset + size > len(packet):
            raise ValueError("truncated OSC value")
        value = struct.unpack_from(fmt, packet, offset)[0]
        offset += size
        return value

    address = read_string()
    if address == "#bundle":
        timetag = read_number(">Q", 8)
        elements = []
        while offset < len(packet):
            size = read_number(">i", 4)
            if size <= 0 or offset + size > len(packet):
                raise ValueError("invalid OSC bundle element size")
            elements.append(decode(packet[offset:offset + size], _depth=_depth + 1))
            offset += size
        return Bundle(timetag, tuple(elements))
    if not address.startswith("/"):
        raise ValueError("invalid OSC address")
    tags = read_string()
    if not tags.startswith(","):
        raise ValueError("missing OSC type tag prefix")
    arguments = []
    for tag in tags[1:]:
        if tag == "i":
            arguments.append(read_number(">i", 4))
        elif tag == "f":
            arguments.append(read_number(">f", 4))
        elif tag == "s":
            arguments.append(read_string())
        elif tag in "TF":
            arguments.append(tag == "T")
        else:
            raise ValueError(f"unsupported OSC type tag: {tag}")
    if offset != len(packet):
        raise ValueError("trailing OSC payload")
    return Message(address, tuple(arguments))


class OscUdp:
    """Explicit IPv4 endpoint. Local port 0 selects an ephemeral port.

    An OSC peer may require a fixed reply port or registration. Neither is
    inferred here. Caller must configure both according to its manual.
    """

    def __init__(self, local_ip: str, local_port: int, timeout: float = 1.0):
        ipaddress.IPv4Address(local_ip)
        if not 0 <= local_port <= 65535:
            raise ValueError("invalid local UDP port")
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be finite and positive")
        self._socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            self._socket.settimeout(timeout)
            self._socket.bind((local_ip, local_port))
        except BaseException:
            self._socket.close()
            raise

    @property
    def local_address(self) -> tuple[str, int]:
        return self._socket.getsockname()

    def send(self, remote_ip: str, remote_port: int, message: Message) -> None:
        remote = ipaddress.IPv4Address(remote_ip)
        if remote.is_multicast or remote.is_unspecified or remote == ipaddress.IPv4Address("255.255.255.255"):
            raise ValueError("an explicit unicast destination is required")
        if not 1 <= remote_port <= 65535:
            raise ValueError("invalid destination UDP port")
        self._socket.sendto(encode(message), (str(remote), remote_port))

    def receive(self) -> tuple[Message | Bundle, tuple[str, int]]:
        packet, sender = self._socket.recvfrom(65535)
        return decode(packet), sender

    def close(self) -> None:
        self._socket.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def argument(text: str) -> int | float | str | bool:
    try:
        tag, value = text.split(":", 1)
        if tag == "i":
            return int(value)
        if tag == "f":
            return float(value)
        if tag == "s":
            return value
        if tag == "b" and value in ("true", "false"):
            return value == "true"
    except ValueError:
        pass
    raise argparse.ArgumentTypeError("use i:123, f:0.5, s:text or b:true/false")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local-ip", default="127.0.0.1")
    parser.add_argument("--local-port", type=int, default=0)
    parser.add_argument("--timeout", type=float, default=1.0)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("listen", help="receive only; never transmits")
    send = commands.add_parser("send", help="send one explicitly specified OSC message")
    send.add_argument("--remote-ip", required=True)
    send.add_argument("--remote-port", type=int, required=True)
    send.add_argument("--address", required=True)
    send.add_argument("--arg", type=argument, action="append", default=[])
    args = parser.parse_args()
    try:
        with OscUdp(args.local_ip, args.local_port, args.timeout) as client:
            if args.command == "send":
                client.send(args.remote_ip, args.remote_port, Message(args.address, tuple(args.arg)))
                print("UDP datagram sent; CueMix acceptance is NOT confirmed.")
                return 0
            print(f"Listening on {client.local_address}; Ctrl+C to stop", flush=True)
            while True:
                try:
                    packet, sender = client.receive()
                    print(f"{sender}: {packet!r}", flush=True)
                except socket.timeout:
                    continue
                except ValueError as error:
                    print(f"Ignored unsupported/malformed OSC: {error}", flush=True)
    except KeyboardInterrupt:
        return 0
    except (OSError, ValueError) as error:
        parser.exit(1, f"OSC error: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
