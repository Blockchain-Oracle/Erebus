"use client";

import { useState } from "react";

/** Largest file either tool will read. An audit report or an index is a few kilobytes. */
const LIMIT = 1024 * 1024;

/**
 * A drop zone that reads one file in this tab. The file never leaves the browser: it is read
 * with `File.text()`/`arrayBuffer()` and handed to the caller; nothing is sent anywhere.
 * The label wraps the input, so it is a native, keyboard-reachable file picker.
 */
export function FileDrop({
  id,
  accept,
  prompt,
  onText,
  onBytes,
  onError,
}: {
  id: string;
  accept: string;
  prompt: string;
  onText?: (text: string, name: string) => void;
  onBytes?: (bytes: ArrayBuffer, name: string) => void;
  onError: (message: string) => void;
}) {
  const [over, setOver] = useState(false);

  const read = async (file: File | undefined) => {
    if (!file) return;
    if (file.size > LIMIT) {
      onError("That file is larger than 1 MB, which is far bigger than this kind of file.");
      return;
    }
    try {
      if (onBytes) onBytes(await file.arrayBuffer(), file.name);
      if (onText) onText(await file.text(), file.name);
    } catch {
      onError("That file could not be read.");
    }
  };

  return (
    <label
      htmlFor={id}
      onDragOver={(event) => {
        event.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => {
        event.preventDefault();
        setOver(false);
        void read(event.dataTransfer.files[0]);
      }}
      className={`plate flex min-h-[9rem] cursor-pointer flex-col items-start justify-end gap-3 border p-5 transition-colors duration-200 ${
        over ? "border-fore" : "border-rule-2 hover:border-fore-3"
      }`}
    >
      <span className="label">{prompt}</span>
      <span className="mono-xs text-fore-3">read in this tab · never uploaded</span>
      <input
        id={id}
        type="file"
        accept={accept}
        className="sr-only"
        onChange={(event) => {
          void read(event.target.files?.[0]);
          event.target.value = "";
        }}
      />
    </label>
  );
}
