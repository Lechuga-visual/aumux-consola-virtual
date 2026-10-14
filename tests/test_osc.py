import socket
import struct
import unittest

from tools.cuemix_osc import Bundle, Message, OscUdp, decode, encode


class OscTests(unittest.TestCase):
    def test_known_wire_format(self):
        # Independent OSC 1.0 wire fixture: padded address/tags, big endian.
        wire = b"/test\0\0\0,ifsTF\0\0" + b"\x00\x00\x00\x07\x3f\x00\x00\x00hi\0\0"
        message = Message("/test", (7, 0.5, "hi", True, False))
        self.assertEqual(decode(wire), message)
        self.assertEqual(encode(message), wire)

    def test_bundle_feedback(self):
        wire = b"/mute\0\0\0,i\0\0\x00\x00\x00\x01"
        bundle = b"#bundle\0" + struct.pack(">Qi", 1, len(wire)) + wire
        self.assertEqual(decode(bundle), Bundle(1, (Message("/mute", (1,)),)))

    def test_malformed_packets_rejected(self):
        for wire in (b"", b"/x\0", b"/x\0\x01,i\0\0\0\0\0\0",
                     b"/x\0\0,f\0\0", b"/x\0\0,b\0\0",
                     b"/x\0\0,\0\0\0junk",
                     b"#bundle\0" + struct.pack(">Qi", 1, 100)):
            with self.subTest(wire=wire), self.assertRaises(ValueError):
                decode(wire)

    def test_controls_validated(self):
        for message in (Message("no-slash"), Message("/x", (float("nan"),)),
                        Message("/x", (2**31,)), Message("/x", ("bad\0value",))):
            with self.subTest(message=message), self.assertRaises(ValueError):
                encode(message)

    def test_udp_exchange_with_independent_peer(self):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer:
            peer.bind(("127.0.0.1", 0))
            peer.settimeout(1.0)
            with OscUdp("127.0.0.1", 0) as client:
                # Creating or using receive is not allowed to emit messages.
                client.send(*peer.getsockname(), Message("/gain", (0.5,)))
                request, sender = peer.recvfrom(1024)
                self.assertEqual(request, b"/gain\0\0\0,f\0\0\x3f\x00\x00\x00")
                self.assertEqual(sender, client.local_address)
                peer.sendto(b"/mute\0\0\0,i\0\0\x00\x00\x00\x01", sender)
                feedback, source = client.receive()
                self.assertEqual(feedback, Message("/mute", (1,)))
                self.assertEqual(source, peer.getsockname())

    def test_listen_does_not_send_and_times_out(self):
        with OscUdp("127.0.0.1", 0, timeout=0.01) as client:
            with self.assertRaises(socket.timeout):
                client.receive()


if __name__ == "__main__":
    unittest.main()
