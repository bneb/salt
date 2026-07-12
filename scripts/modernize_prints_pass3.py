#!/usr/bin/env python3
"""
Third pass: Handle remaining puts("literal") calls.
These are inline puts with string literals that weren't caught by
the first two passes because they use patterns like puts("msg\n")
without the \0 terminator.
"""
import re
import sys
import os

SKIP_FILES = {'option.salt', 'result.salt'}

def transform_file(filepath):
    basename = os.path.basename(filepath)
    if basename in SKIP_FILES:
        return False, "skipped"
    
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
        
        # Remove leftover extern fn declarations
        if re.match(r'^extern fn (puts|printf_shim|print_i64)\b', stripped):
            changes += 1
            continue
        
        # puts("msg\n") → println("msg")  — inline on same line as other code
        # Handle: if cond { puts("msg\n"); result = 1; }
        m = re.search(r'puts\("([^"]*?)\\n"\)', line)
        if m:
            msg = m.group(1)
            new_line = line[:m.start()] + f'println("{msg}")' + line[m.end():]
            matched = True
            changes += 1
        
        # puts("msg") without \n → print("msg")
        if not matched:
            m = re.search(r'puts\("([^"]*?)"\)', line)
            if m:
                msg = m.group(1)
                if '\\n' not in msg:
                    new_line = line[:m.start()] + f'print("{msg}")' + line[m.end():]
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
    
    all_files = find_salt_files(os.path.join(root, 'salt-front/tests'))
    all_files += find_salt_files(os.path.join(root, 'tests'))
    all_files += find_salt_files(os.path.join(root, 'benchmarks'))
    
    changed = 0
    for fpath in sorted(set(all_files)):
        ok, desc = transform_file(fpath)
        if ok:
            print(f"  ✓ {os.path.relpath(fpath, root)}: {desc}")
            changed += 1
    
    print(f"\nThird pass: modernized {changed} files.")
