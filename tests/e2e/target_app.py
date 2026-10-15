"""A tiny Tk window that records every input event as JSON lines.

Used only by the end-to-end tests: pointhand drives this window and the test
reads the log back to confirm that clicks, keys, drags and scrolls really land.
"""

import ctypes
import json
import sys
import time
import tkinter as tk

try:
    ctypes.WinDLL("shcore").SetProcessDpiAwareness(2)
except OSError:
    pass

LOG_PATH = sys.argv[1]


def log(kind: str, **data) -> None:
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps({"t": time.time(), "kind": kind, **data}) + "\n")


root = tk.Tk()
root.title("pointhand-e2e-target")
root.geometry("640x420+120+120")
root.attributes("-topmost", True)
entry = tk.Entry(root, font=("Segoe UI", 16))
entry.pack(fill="x", padx=10, pady=10)
canvas = tk.Canvas(root, bg="#222244", highlightthickness=0)
canvas.pack(fill="both", expand=True, padx=10, pady=(0, 10))

# bind_all lives on a separate binding tag, so it sees EVERY press even when
# Tk promotes rapid clicks to <Double-Button-1> / <Triple-Button-1> on the canvas.
root.bind_all("<ButtonPress-1>", lambda e: log("press", x=e.x_root, y=e.y_root))
canvas.bind("<Button-1>", lambda e: log("click", x=e.x_root, y=e.y_root))
canvas.bind("<Double-Button-1>", lambda e: log("double", x=e.x_root, y=e.y_root))
canvas.bind("<Button-3>", lambda e: log("right", x=e.x_root, y=e.y_root))
canvas.bind("<B1-Motion>", lambda e: log("motion", x=e.x_root, y=e.y_root))
def released(event) -> None:
    log("release", x=event.x_root, y=event.y_root)
    root.after(80, lambda: log("button_state", left_down=bool(ctypes.windll.user32.GetAsyncKeyState(1) & 0x8000)))


canvas.bind("<ButtonRelease-1>", released)
canvas.bind("<MouseWheel>", lambda e: log("wheel", delta=e.delta))
entry.bind("<KeyRelease>", lambda e: log("text", value=entry.get()))


def announce() -> None:
    root.update_idletasks()
    log("ready",
        entry=[entry.winfo_rootx() + entry.winfo_width() // 2, entry.winfo_rooty() + entry.winfo_height() // 2],
        canvas=[canvas.winfo_rootx(), canvas.winfo_rooty(), canvas.winfo_width(), canvas.winfo_height()])
    entry.focus_force()


root.after(400, announce)
root.mainloop()
