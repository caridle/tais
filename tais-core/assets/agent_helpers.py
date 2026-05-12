# TAIS Agent Helpers — prepended to every Python execution
# Provides file operations and user interaction without external deps.

import os, re, difflib, itertools, collections

def file_read(path, start=1, count=200, keyword=None, show_linenos=True):
    """Read file with line numbers and optional keyword search.
    Returns formatted string with line numbers like: 42|content"""
    try:
        with open(path, 'r', encoding='utf-8', errors='replace') as f:
            lines = f.readlines()
        total = len(lines)
        if keyword:
            for i, line in enumerate(lines):
                if keyword.lower() in line.lower():
                    start = max(1, i - count//3)
                    break
        start = max(1, start)
        end = min(total, start + count - 1)
        result = [f"{i}|{lines[i-1].rstrip()}" for i in range(start, end+1)]
        header = f"[FILE] {total} lines, showing {start}-{end}"
        if end < total:
            header += " (partial)"
        return header + "\n" + "\n".join(result)
    except FileNotFoundError:
        # Fuzzy search for similar filenames
        dirname = os.path.dirname(os.path.abspath(path)) or '.'
        base = os.path.basename(path)
        candidates = []
        for root, dirs, files in os.walk(dirname):
            for f in files:
                ratio = difflib.SequenceMatcher(None, base.lower(), f.lower()).ratio()
                if ratio > 0.3:
                    candidates.append((ratio, os.path.join(root, f)))
            if len(candidates) > 10:
                break
        candidates.sort(key=lambda x: -x[0])
        msg = f"File not found: {path}"
        if candidates:
            msg += "\n\nDid you mean:\n"
            for ratio, c in candidates[:5]:
                msg += f"  {c} ({ratio:.0%})\n"
        return msg
    except Exception as e:
        return f"Error reading {path}: {e}"

def file_write(path, content, mode='overwrite'):
    """Write/append/prepend content to a file.
    mode: 'overwrite' (default), 'append', 'prepend'"""
    try:
        os.makedirs(os.path.dirname(os.path.abspath(path)) or '.', exist_ok=True)
        if mode == 'append' and os.path.exists(path):
            with open(path, 'a', encoding='utf-8') as f:
                f.write(content)
        elif mode == 'prepend' and os.path.exists(path):
            old = open(path, 'r', encoding='utf-8').read()
            with open(path, 'w', encoding='utf-8') as f:
                f.write(content + old)
        else:
            with open(path, 'w', encoding='utf-8') as f:
                f.write(content)
        return f"✅ Wrote {len(content)} bytes to {path} (mode={mode})"
    except Exception as e:
        return f"❌ Failed to write {path}: {e}"

def file_patch(path, old_content, new_content):
    """Replace a UNIQUE text block in a file. Fails if not unique."""
    try:
        if not os.path.exists(path):
            return f"❌ File not found: {path}"
        with open(path, 'r', encoding='utf-8') as f:
            full = f.read()
        count = full.count(old_content)
        if count == 0:
            return f"❌ Old content not found in {path}. Use file_read() to check current content."
        if count > 1:
            return f"❌ Found {count} matches — not unique. Provide a longer/more specific old_content."
        updated = full.replace(old_content, new_content)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(updated)
        return f"✅ Patched {path} successfully"
    except Exception as e:
        return f"❌ Patch failed: {e}"

def ask_user(question, candidates=None):
    """Ask the user a question. Returns the user's answer (or raises if not interactive).
    In TAIS agent context, this interrupts execution and prompts the user."""
    msg = f"[ASK_USER] {question}"
    if candidates:
        msg += f"\nOptions: {', '.join(candidates)}"
    print(msg)
    # In TAIS, this is intercepted by the agent loop
    return "__TAIS_ASK_USER__:" + question
