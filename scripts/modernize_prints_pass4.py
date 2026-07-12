#!/usr/bin/env python3
"""
Fourth pass: Transform remaining print externs:
  printf_str("msg\n") → println("msg")
  printf_str("msg") → print("msg")
  printf_i64("fmt %lld\n", val) → println(f"fmt {val}")
  print_f32("fmt %f\n", val) → println(f"fmt {val}")
  print_f32("fmt %0.1f\n", val) → println(f"fmt {val}")
Also removes the corresponding extern fn declarations.
"""
import re
import sys
import os

def transform_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()
    
    lines = content.split('\n')
    new_lines = []
    changes = 0
    
    for line in lines:
        stripped = line.strip()
        indent = re.match(r'^(\s*)', line).group(1)
        new_line = line
        matched = False
        
        # --- Remove extern fn declarations for these print functions ---
        if re.match(r'^extern fn (printf_str|printf_i64|print_f32)\b', stripped):
            changes += 1
            continue
        
        # --- printf_str("msg\n") → println("msg") ---
        if not matched:
            m = re.search(r'printf_str\("([^"]*?)\\n"\)', line)
            if m:
                msg = m.group(1)
                # Handle escaped newlines within the message
                new_line = line[:m.start()] + f'println("{msg}")' + line[m.end():]
                matched = True
                changes += 1
        
        # --- printf_str("msg") without \n → print("msg") ---
        if not matched:
            m = re.search(r'printf_str\("([^"]*?)"\)', line)
            if m:
                msg = m.group(1)
                if '\\n' not in msg:
                    new_line = line[:m.start()] + f'print("{msg}")' + line[m.end():]
                    matched = True
                    changes += 1
        
        # --- printf_i64("fmt %lld\n", val) → println(f"fmt {val}") ---
        if not matched:
            m = re.search(r'printf_i64\("([^"]*?)\\n",\s*(.+?)\)', line)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                fstr = re.sub(r'%l?l?[dxu]', '{' + val + '}', fmt)
                fstr = fstr.replace('%%', '%')
                new_line = line[:m.start()] + f'println(f"{fstr}")' + line[m.end():]
                matched = True
                changes += 1
        
        # --- printf_i64("fmt", val) without \n → print(f"fmt {val}") ---
        if not matched:
            m = re.search(r'printf_i64\("([^"]*?)",\s*(.+?)\)', line)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                fstr = re.sub(r'%l?l?[dxu]', '{' + val + '}', fmt)
                fstr = fstr.replace('%%', '%')
                new_line = line[:m.start()] + f'print(f"{fstr}")' + line[m.end():]
                matched = True
                changes += 1
        
        # --- print_f32("fmt %f\n", val) → println(f"fmt {val}") ---
        if not matched:
            m = re.search(r'print_f32\("([^"]*?)\\n",\s*(.+?)\)', line)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                fstr = re.sub(r'%[\d.]*f', '{' + val + '}', fmt)
                fstr = fstr.replace('%%', '%')
                new_line = line[:m.start()] + f'println(f"{fstr}")' + line[m.end():]
                matched = True
                changes += 1
        
        # --- print_f32("fmt %f", val) without \n → print(f"fmt {val}") ---
        if not matched:
            m = re.search(r'print_f32\("([^"]*?)",\s*(.+?)\)', line)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                fstr = re.sub(r'%[\d.]*f', '{' + val + '}', fmt)
                fstr = fstr.replace('%%', '%')
                new_line = line[:m.start()] + f'print(f"{fstr}")' + line[m.end():]
                matched = True
                changes += 1
        
        new_lines.append(new_line)
    
    if changes == 0:
        return False, "no changes"
    
    result = '\n'.join(new_lines)
    result = re.sub(r'\n{3,}', '\n\n', result)
    
    with open(filepath, 'w') as f:
        f.write(result)
    
    return True, f"{changes} change(s)"

def find_salt_files(root):
    result = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ('target', 'bin', '.git')]
        for fn in sorted(filenames):
            if fn.endswith('.salt'):
                result.append(os.path.join(dirpath, fn))
    return result

if __name__ == '__main__':
    root = sys.argv[1] if len(sys.argv) > 1 else '.'
    
    all_files = []
    for d in ['tests', 'benchmarks', 'examples', 'salt-front/tests']:
        path = os.path.join(root, d)
        if os.path.isdir(path):
            all_files.extend(find_salt_files(path))
    
    changed = 0
    for fpath in sorted(set(all_files)):
        ok, desc = transform_file(fpath)
        if ok:
            print(f"  ✓ {os.path.relpath(fpath, root)}: {desc}")
            changed += 1
    
    print(f"\nFourth pass: modernized {changed} files.")
