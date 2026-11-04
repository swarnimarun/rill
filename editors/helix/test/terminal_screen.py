"""Track the visible cells in Helix's ANSI terminal redraws without extra deps."""
import codecs
import re
import unicodedata


class TerminalScreen:
    def __init__(self, rows, columns):
        self.rows = rows
        self.columns = columns
        self.row = self.column = 0
        self.style = {}
        self.cells = {}
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def feed(self, chunk):
        self.pending += self.decoder.decode(chunk)
        index = 0
        while index < len(self.pending):
            char = self.pending[index]
            if char == "\x1b":
                if index + 1 == len(self.pending):
                    break
                following = self.pending[index + 1]
                if following == "[":
                    match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.pending[index:])
                    if not match:
                        break
                    self.control(match[1], match[3])
                    index += len(match[0])
                    continue
                if following in "]P":
                    end = re.search(r"\x07|\x1b\\", self.pending[index + 2:])
                    if not end:
                        break
                    index += 2 + end.end()
                    continue
                index += 2
                continue
            if char == "\r":
                self.column = 0
            elif char == "\n":
                self.row = min(self.rows - 1, self.row + 1)
            elif char == "\b":
                self.column = max(0, self.column - 1)
            elif char == "\t":
                self.column = min(self.columns - 1, (self.column // 8 + 1) * 8)
            elif char >= " ":
                if self.column >= self.columns:
                    self.row = min(self.rows - 1, self.row + 1)
                    self.column = 0
                if not unicodedata.combining(char):
                    self.cells[self.row, self.column] = (char, dict(self.style))
                    self.column += 2 if unicodedata.east_asian_width(char) in "WF" else 1
            index += 1
        self.pending = self.pending[index:]

    def control(self, raw, command):
        if raw.startswith("?"):
            if raw == "?1049" and command == "h":
                self.cells.clear()
            return
        if raw and not (raw[0].isdigit() or raw[0] == ";"):
            return
        values = []
        for value in raw.split(";"):
            if ":" in value:
                group = [int(part or 0) for part in value.split(":")]
                if group[0] in (38, 48, 58) and group[1] == 2:
                    values.extend([group[0], 2, *group[-3:]])
                elif group[0] == 4:
                    self.style[4] = tuple(group[1:])
                else:
                    self.style[f"sgr_{group[0]}"] = tuple(group[1:])
            else:
                values.append(int(value or 0))
        if not values:
            return
        amount = values[0] or 1
        if command in "Hf":
            self.row = amount - 1
            self.column = (values[1] or 1) - 1 if len(values) > 1 else 0
        elif command == "G":
            self.column = amount - 1
        elif command == "d":
            self.row = amount - 1
        elif command in "ABCD":
            if command == "A": self.row -= amount
            if command == "B": self.row += amount
            if command == "C": self.column += amount
            if command == "D": self.column -= amount
        elif command in "JK":
            for row, column in list(self.cells):
                relative = (row, column) if command == "J" else column
                cursor = (self.row, self.column) if command == "J" else self.column
                if command == "K" and row != self.row:
                    continue
                if values[0] == 2 or (values[0] == 0 and relative >= cursor) or (values[0] == 1 and relative <= cursor):
                    del self.cells[row, column]
        elif command == "m":
            index = 0
            while index < len(values):
                value = values[index]
                if value == 0:
                    self.style.clear()
                elif value in (38, 48, 58) and index + 1 < len(values):
                    count = 4 if values[index + 1] == 2 else 2
                    self.style[{38: "foreground", 48: "background", 58: "underline_color"}[value]] = tuple(values[index + 1:index + count + 1])
                    index += count
                elif 30 <= value <= 37 or 90 <= value <= 97:
                    self.style["foreground"] = value
                elif 40 <= value <= 47 or 100 <= value <= 107:
                    self.style["background"] = value
                elif value in (39, 49, 59):
                    self.style.pop({39: "foreground", 49: "background", 59: "underline_color"}[value], None)
                elif value in (22, 23, 24, 27, 29):
                    for removed in {22: (1, 2), 23: (3,), 24: (4,), 27: (7,), 29: (9,)}[value]:
                        self.style.pop(removed, None)
                else:
                    self.style[value] = True
                index += 1
        self.row = min(self.rows - 1, max(0, self.row))
        self.column = min(self.columns, max(0, self.column))

    def lines(self):
        return ["".join(self.cells.get((row, column), (" ", {}))[0]
                        for column in range(self.columns)).rstrip() for row in range(self.rows)]

    def text(self):
        return "\n".join(self.lines())

    def styled_text(self, text, standalone=False):
        for row, line in enumerate(self.lines()):
            if (column := line.find(text)) >= 0:
                if standalone and line[column + len(text):].strip(" │"):
                    continue
                return [self.cells.get((row, column + offset), (" ", {}))[1] for offset in range(len(text))]
        return None
