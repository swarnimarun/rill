#!/usr/bin/env python3
"""Check visible terminal evidence, including redraws and active parameters."""
import unittest
from terminal_screen import TerminalScreen


class TerminalScreenTests(unittest.TestCase):
    def test_overwritten_and_erased_text_is_not_visible_evidence(self):
        screen = TerminalScreen(4, 60)
        screen.feed(b"answer: i32\r\x1b[2Kanswer")
        self.assertNotIn(": i32", screen.text())
        self.assertIn("answer", screen.text())
        screen.feed(b"\x1b[2J")
        self.assertFalse(screen.text().strip())

    def test_chunked_unicode_and_escapes_keep_screen_positions(self):
        screen = TerminalScreen(4, 60)
        for byte in "\x1b[2;3H😀 answer: i32".encode():
            screen.feed(bytes([byte]))
        self.assertIn("😀  answer: i32", screen.lines()[1])
        self.assertFalse(screen.lines()[0])

    def test_signature_parameter_styles_advance_with_redraws(self):
        screen = TerminalScreen(4, 60)
        label = "fn add(x: i32, y: i32) -> i32"
        active = b"\x1b[48;2;58;61;65m"
        inactive = b"\x1b[48;2;37;37;38m"
        screen.feed(b"\x1b[2;1H" + inactive + b"fn add(" + active + b"x: i32" + inactive + b", y: i32) -> i32")
        first = screen.styled_text(label, standalone=True)
        screen.feed(b"\x1b[2;1H\x1b[2K" + inactive + b"fn add(x: i32, " + active + b"y: i32" + inactive + b") -> i32")
        second = screen.styled_text(label, standalone=True)
        x, y = label.index("x:"), label.index("y:")
        self.assertNotEqual(first[x], first[y])
        self.assertEqual((first[x], first[y]), (second[y], second[x]))

    def test_source_function_text_cannot_pass_a_popup_check(self):
        screen = TerminalScreen(4, 60)
        label = "fn add(x: i32, y: i32) -> i32"
        screen.feed((label + " { x + y }").encode())
        self.assertIsNone(screen.styled_text(label, standalone=True))
        screen.feed(("\x1b[3;1H" + label).encode())
        self.assertIsNotNone(screen.styled_text(label, standalone=True))

    def test_extended_underline_color_and_style_do_not_leak_after_reset(self):
        screen = TerminalScreen(4, 60)
        screen.feed(b"\x1b[58:2::255:18:18m\x1b[4:3merror\x1b[24;59mok")
        self.assertIn(4, screen.styled_text("error")[0])
        self.assertNotIn(4, screen.styled_text("ok")[0])
        self.assertNotIn("underline_color", screen.styled_text("ok")[0])


if __name__ == "__main__":
    unittest.main()
