from __future__ import annotations

import csv
import hashlib
import os
import queue
import threading
import time
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import tkinter as tk
from tkinter import filedialog, messagebox, ttk


# =========================
# Data models
# =========================

@dataclass
class FileEntry:
    path: Path
    size: int


@dataclass
class DuplicateResult:
    group_id: int
    size: int
    hash_value: str
    path: Path


# =========================
# Core logic
# =========================

def iter_files(root_dir: Path) -> List[FileEntry]:
    """Recursively collect files under root_dir."""
    files: List[FileEntry] = []
    for root, _, filenames in os.walk(root_dir):
        for name in filenames:
            p = Path(root) / name
            try:
                if p.is_file():
                    files.append(FileEntry(path=p, size=p.stat().st_size))
            except (OSError, PermissionError):
                # Skip files that cannot be accessed
                continue
    return files


def calc_file_hash(
    file_path: Path,
    algorithm: str = "sha256",
    chunk_size: int = 1024 * 1024,
    cancel_event: Optional[threading.Event] = None,
) -> str:
    """Calculate hash for a file in chunks."""
    h = hashlib.new(algorithm)
    with file_path.open("rb") as f:
        while True:
            if cancel_event is not None and cancel_event.is_set():
                raise RuntimeError("Cancelled")
            chunk = f.read(chunk_size)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def format_seconds(seconds: float) -> str:
    """Human-readable time string."""
    if seconds < 0 or seconds == float("inf"):
        return "--:--:--"
    total = int(seconds)
    h = total // 3600
    m = (total % 3600) // 60
    s = total % 60
    return f"{h:02d}:{m:02d}:{s:02d}"


# =========================
# Worker thread
# =========================

class DuplicateScanWorker(threading.Thread):
    def __init__(
        self,
        root_dir: Path,
        output_csv: Path,
        event_queue: "queue.Queue[Tuple[str, dict]]",
        cancel_event: threading.Event,
        hash_algorithm: str = "sha256",
    ) -> None:
        super().__init__(daemon=True)
        self.root_dir = root_dir
        self.output_csv = output_csv
        self.event_queue = event_queue
        self.cancel_event = cancel_event
        self.hash_algorithm = hash_algorithm

    def _send(self, event_type: str, **payload: object) -> None:
        self.event_queue.put((event_type, payload))

    def run(self) -> None:
        start_time = time.time()
        try:
            self._send("status", message="ファイル一覧を収集中...")
            files = iter_files(self.root_dir)

            if self.cancel_event.is_set():
                self._send("cancelled", message="中止されました。")
                return

            total_files = len(files)
            total_bytes = sum(f.size for f in files)

            self._send(
                "scan_summary",
                total_files=total_files,
                total_bytes=total_bytes,
            )

            if total_files == 0:
                self._send("done", message="ファイルが見つかりませんでした。", results=[])
                return

            # Step 1: group by file size (fast prefilter)
            self._send("status", message="サイズでグループ化中...")
            size_map: Dict[int, List[FileEntry]] = {}
            for i, file_entry in enumerate(files, start=1):
                if self.cancel_event.is_set():
                    self._send("cancelled", message="中止されました。")
                    return

                size_map.setdefault(file_entry.size, []).append(file_entry)

                # Progress for phase 1 (0 - 20%)
                progress_ratio = i / total_files if total_files else 1.0
                self._send(
                    "progress",
                    phase="size_grouping",
                    percent=progress_ratio * 20.0,
                    processed_files=i,
                    total_files=total_files,
                    processed_bytes=0,
                    total_bytes=total_bytes,
                    elapsed=time.time() - start_time,
                    eta=float("inf"),
                )

            target_groups = [group for group in size_map.values() if len(group) >= 2]
            candidate_files = sum(len(group) for group in target_groups)

            self._send(
                "status",
                message=f"ハッシュ計算中...（候補 {candidate_files} 件）",
            )

            # Step 2: hash only same-size candidates
            hash_map: Dict[Tuple[int, str], List[Path]] = {}
            processed_candidate_files = 0
            processed_candidate_bytes = 0
            total_candidate_bytes = sum(f.size for g in target_groups for f in g)

            for group in target_groups:
                for file_entry in group:
                    if self.cancel_event.is_set():
                        self._send("cancelled", message="中止されました。")
                        return

                    try:
                        hash_value = calc_file_hash(
                            file_entry.path,
                            algorithm=self.hash_algorithm,
                            cancel_event=self.cancel_event,
                        )
                    except (OSError, PermissionError):
                        continue
                    except RuntimeError:
                        self._send("cancelled", message="中止されました。")
                        return

                    key = (file_entry.size, hash_value)
                    hash_map.setdefault(key, []).append(file_entry.path)

                    processed_candidate_files += 1
                    processed_candidate_bytes += file_entry.size

                    elapsed = time.time() - start_time
                    if processed_candidate_bytes > 0 and total_candidate_bytes > 0:
                        speed = processed_candidate_bytes / max(elapsed, 0.001)
                        remaining_bytes = total_candidate_bytes - processed_candidate_bytes
                        eta = remaining_bytes / max(speed, 1.0)
                    else:
                        eta = float("inf")

                    # Progress for phase 2 (20 - 100%)
                    phase_ratio = (
                        processed_candidate_files / candidate_files
                        if candidate_files > 0
                        else 1.0
                    )
                    self._send(
                        "progress",
                        phase="hashing",
                        percent=20.0 + (phase_ratio * 80.0),
                        processed_files=processed_candidate_files,
                        total_files=max(candidate_files, 1),
                        processed_bytes=processed_candidate_bytes,
                        total_bytes=max(total_candidate_bytes, 1),
                        elapsed=elapsed,
                        eta=eta,
                    )

            # Build duplicate results
            duplicate_results: List[DuplicateResult] = []
            group_id = 1
            for (size, hash_value), paths in sorted(hash_map.items(), key=lambda x: (x[0][0], x[0][1])):
                if len(paths) >= 2:
                    for p in sorted(paths):
                        duplicate_results.append(
                            DuplicateResult(
                                group_id=group_id,
                                size=size,
                                hash_value=hash_value,
                                path=p,
                            )
                        )
                    group_id += 1

            # Save CSV
            self._send("status", message="CSVを書き出し中...")
            self.output_csv.parent.mkdir(parents=True, exist_ok=True)
            with self.output_csv.open("w", newline="", encoding="utf-8-sig") as f:
                writer = csv.writer(f)
                writer.writerow(
                    [
                        "group_id",
                        "size_bytes",
                        "hash_algorithm",
                        "hash_value",
                        "file_path",
                    ]
                )
                for row in duplicate_results:
                    writer.writerow(
                        [
                            row.group_id,
                            row.size,
                            self.hash_algorithm,
                            row.hash_value,
                            str(row.path),
                        ]
                    )

            elapsed_total = time.time() - start_time
            self._send(
                "done",
                message=f"完了: 重複グループ {group_id - 1} 件 / 重複ファイル {len(duplicate_results)} 件",
                results=duplicate_results,
                elapsed=elapsed_total,
            )

        except Exception as e:
            self._send("error", message=f"エラー: {e}")


# =========================
# GUI
# =========================

class DuplicateFinderApp:
    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        self.root.title("重複ファイル探索ツール（非破壊）")
        self.root.geometry("980x700")

        self.event_queue: "queue.Queue[Tuple[str, dict]]" = queue.Queue()
        self.cancel_event = threading.Event()
        self.worker: Optional[DuplicateScanWorker] = None
        self.current_results: List[DuplicateResult] = []

        # Variables
        self.scan_path_var = tk.StringVar()
        self.csv_path_var = tk.StringVar()
        self.status_var = tk.StringVar(value="待機中")
        self.progress_var = tk.DoubleVar(value=0.0)
        self.meta_var = tk.StringVar(value="")

        self._build_ui()
        self._poll_events()

    def _build_ui(self) -> None:
        # Top frame
        top = ttk.Frame(self.root, padding=10)
        top.pack(fill="x")

        ttk.Label(top, text="参照フォルダ").grid(row=0, column=0, sticky="w")
        ttk.Entry(top, textvariable=self.scan_path_var, width=90).grid(row=0, column=1, padx=5, sticky="we")
        ttk.Button(top, text="参照...", command=self._choose_scan_folder).grid(row=0, column=2, padx=5)

        ttk.Label(top, text="CSV出力先").grid(row=1, column=0, sticky="w", pady=(8, 0))
        ttk.Entry(top, textvariable=self.csv_path_var, width=90).grid(row=1, column=1, padx=5, pady=(8, 0), sticky="we")
        ttk.Button(top, text="保存先...", command=self._choose_csv_path).grid(row=1, column=2, padx=5, pady=(8, 0))

        top.columnconfigure(1, weight=1)

        # Buttons
        btn_frame = ttk.Frame(self.root, padding=(10, 0, 10, 10))
        btn_frame.pack(fill="x")

        self.run_btn = ttk.Button(btn_frame, text="実行", command=self._start_scan)
        self.run_btn.pack(side="left")

        self.cancel_btn = ttk.Button(btn_frame, text="中止", command=self._cancel_scan, state="disabled")
        self.cancel_btn.pack(side="left", padx=8)

        self.copy_btn = ttk.Button(btn_frame, text="選択パスをコピー", command=self._copy_selected_path)
        self.copy_btn.pack(side="left")

        # Progress
        prog_frame = ttk.Frame(self.root, padding=(10, 0, 10, 10))
        prog_frame.pack(fill="x")

        ttk.Label(prog_frame, textvariable=self.status_var).pack(anchor="w")
        ttk.Progressbar(prog_frame, variable=self.progress_var, maximum=100.0).pack(fill="x", pady=5)
        ttk.Label(prog_frame, textvariable=self.meta_var).pack(anchor="w")

        # Result table
        table_frame = ttk.Frame(self.root, padding=(10, 0, 10, 10))
        table_frame.pack(fill="both", expand=True)

        columns = ("group_id", "size", "hash", "path")
        self.tree = ttk.Treeview(table_frame, columns=columns, show="headings")
        self.tree.heading("group_id", text="Group")
        self.tree.heading("size", text="Size(bytes)")
        self.tree.heading("hash", text="Hash(先頭16桁)")
        self.tree.heading("path", text="Path")

        self.tree.column("group_id", width=70, anchor="center")
        self.tree.column("size", width=110, anchor="e")
        self.tree.column("hash", width=170, anchor="w")
        self.tree.column("path", width=600, anchor="w")

        y_scroll = ttk.Scrollbar(table_frame, orient="vertical", command=self.tree.yview)
        x_scroll = ttk.Scrollbar(table_frame, orient="horizontal", command=self.tree.xview)
        self.tree.configure(yscrollcommand=y_scroll.set, xscrollcommand=x_scroll.set)

        self.tree.grid(row=0, column=0, sticky="nsew")
        y_scroll.grid(row=0, column=1, sticky="ns")
        x_scroll.grid(row=1, column=0, sticky="ew")
        self.tree.bind("<Double-1>", self._open_selected_in_explorer)

        table_frame.rowconfigure(0, weight=1)
        table_frame.columnconfigure(0, weight=1)

        # Default CSV path
        default_csv = Path.cwd() / "duplicate_report.csv"
        self.csv_path_var.set(str(default_csv))

    def _choose_scan_folder(self) -> None:
        selected = filedialog.askdirectory(title="参照フォルダを選択")
        if selected:
            self.scan_path_var.set(selected)

    def _choose_csv_path(self) -> None:
        selected = filedialog.asksaveasfilename(
            title="CSV出力先を選択",
            defaultextension=".csv",
            filetypes=[("CSV files", "*.csv"), ("All files", "*.*")],
        )
        if selected:
            self.csv_path_var.set(selected)

    def _start_scan(self) -> None:
        if self.worker is not None and self.worker.is_alive():
            messagebox.showwarning("実行中", "すでに処理中です。")
            return

        scan_path = Path(self.scan_path_var.get().strip())
        csv_path = Path(self.csv_path_var.get().strip())

        if not scan_path.exists() or not scan_path.is_dir():
            messagebox.showerror("入力エラー", "有効な参照フォルダを指定してください。")
            return

        if not csv_path.name:
            messagebox.showerror("入力エラー", "有効なCSV出力先を指定してください。")
            return

        self.cancel_event.clear()
        self.current_results.clear()
        self._clear_table()

        self.progress_var.set(0.0)
        self.status_var.set("開始準備中...")
        self.meta_var.set("")

        self.run_btn.config(state="disabled")
        self.cancel_btn.config(state="normal")

        self.worker = DuplicateScanWorker(
            root_dir=scan_path,
            output_csv=csv_path,
            event_queue=self.event_queue,
            cancel_event=self.cancel_event,
            hash_algorithm="sha256",
        )
        self.worker.start()

    def _cancel_scan(self) -> None:
        if self.worker is not None and self.worker.is_alive():
            self.cancel_event.set()
            self.status_var.set("中止要求を送信しました...")

    def _clear_table(self) -> None:
        for item in self.tree.get_children():
            self.tree.delete(item)

    def _insert_results(self, results: List[DuplicateResult]) -> None:
        self._clear_table()
        for r in results:
            self.tree.insert(
                "",
                "end",
                values=(r.group_id, r.size, r.hash_value[:16], str(r.path)),
            )

    def _copy_selected_path(self) -> None:
        selected = self.tree.selection()
        if not selected:
            messagebox.showinfo("情報", "行を選択してください。")
            return

        # 複数選択対応（改行区切り）
        paths: List[str] = []
        for item in selected:
            values = self.tree.item(item, "values")
            if len(values) >= 4:
                paths.append(str(values[3]))

        text = "\n".join(paths)
        self.root.clipboard_clear()
        self.root.clipboard_append(text)
        self.root.update()
        self.status_var.set(f"{len(paths)} 件のパスをクリップボードにコピーしました。")

    def _open_selected_in_explorer(self, event: tk.Event) -> None:
        # ダブルクリックされた行を取得
        item_id = self.tree.identify_row(event.y)
        if not item_id:
            return

        values = self.tree.item(item_id, "values")
        if len(values) < 4:
            return

        file_path = Path(str(values[3]))

        if not file_path.exists():
            messagebox.showwarning("ファイルなし", f"ファイルが見つかりません:\n{file_path}")
            return

        try:
            # Windows Explorer でファイル選択状態で開く
            subprocess.run(
                ["explorer", "/select,", str(file_path)],
                check=False,
            )
        except Exception as e:
            messagebox.showerror("エラー", f"エクスプローラーを開けませんでした:\n{e}")

    def _poll_events(self) -> None:
        try:
            while True:
                event_type, payload = self.event_queue.get_nowait()
                self._handle_event(event_type, payload)
        except queue.Empty:
            pass
        finally:
            self.root.after(150, self._poll_events)

    def _handle_event(self, event_type: str, payload: dict) -> None:
        if event_type == "status":
            self.status_var.set(str(payload.get("message", "")))

        elif event_type == "scan_summary":
            total_files = int(payload.get("total_files", 0))
            total_bytes = int(payload.get("total_bytes", 0))
            self.meta_var.set(
                f"検出ファイル数: {total_files:,} 件 / 合計容量: {total_bytes:,} bytes"
            )

        elif event_type == "progress":
            percent = float(payload.get("percent", 0.0))
            processed_files = int(payload.get("processed_files", 0))
            total_files = int(payload.get("total_files", 1))
            processed_bytes = int(payload.get("processed_bytes", 0))
            total_bytes = int(payload.get("total_bytes", 1))
            elapsed = float(payload.get("elapsed", 0.0))
            eta = float(payload.get("eta", float("inf")))
            phase = str(payload.get("phase", ""))

            self.progress_var.set(percent)

            if phase == "size_grouping":
                phase_label = "サイズ判定"
            else:
                phase_label = "ハッシュ計算"

            self.meta_var.set(
                f"{phase_label} | {processed_files:,}/{total_files:,} 件 | "
                f"{processed_bytes:,}/{total_bytes:,} bytes | "
                f"経過 {format_seconds(elapsed)} | 残り目安 {format_seconds(eta)}"
            )

        elif event_type == "done":
            self.run_btn.config(state="normal")
            self.cancel_btn.config(state="disabled")
            self.progress_var.set(100.0)

            message = str(payload.get("message", "完了"))
            elapsed = float(payload.get("elapsed", 0.0))
            self.status_var.set(message)

            results = payload.get("results", [])
            if isinstance(results, list):
                self.current_results = results
                self._insert_results(results)

            self.meta_var.set(f"総処理時間: {format_seconds(elapsed)}")
            messagebox.showinfo("完了", message)

        elif event_type == "cancelled":
            self.run_btn.config(state="normal")
            self.cancel_btn.config(state="disabled")
            self.status_var.set(str(payload.get("message", "中止されました。")))
            self.meta_var.set("")

        elif event_type == "error":
            self.run_btn.config(state="normal")
            self.cancel_btn.config(state="disabled")
            self.status_var.set("エラー")
            self.meta_var.set("")
            messagebox.showerror("エラー", str(payload.get("message", "不明なエラー")))


def main() -> None:
    root = tk.Tk()
    app = DuplicateFinderApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()