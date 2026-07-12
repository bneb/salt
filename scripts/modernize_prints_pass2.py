#!/usr/bin/env python3
"""
Second pass: Transform remaining puts/printf_shim/print_i64 call sites
that the first pass missed. Handles:
  puts("msg\n") without \0
  printf_shim("msg\n", 0) with dummy arg
  print_i64("msg\n", 0) with dummy arg
  print_i64("msg\n", val) 
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
    
    original = content
    lines = content.split('\n')
    new_lines = []
    changes = 0
    
    for line in lines:
        stripped = line.strip()
        indent = re.match(r'^(\s*)', line).group(1)
        new_line = line
        matched = False
        
        # --- Remove remaining extern fn declarations for print functions ---
        if re.match(r'^extern fn (puts|printf_shim|print_i64|printf)\b', stripped):
            changes += 1
            continue
        
        # --- puts patterns ---
        # puts("msg\n") or puts("msg\n"); → println("msg")
        m = re.match(r'puts\("(.*)\\n"\);?$', stripped)
        if m:
            msg = m.group(1)
            # Remove trailing \0 if present
            msg = msg.rstrip('\\').rstrip('0')
            if msg.endswith('\\'):
                msg = msg[:-1]
            new_line = f'{indent}println("{msg}");'
            matched = True
            changes += 1
        
        # puts("msg") without \n → print("msg")
        if not matched:
            m = re.match(r'puts\("([^"]+)"\);?$', stripped)
            if m and '\\n' not in m.group(1):
                msg = m.group(1)
                new_line = f'{indent}print("{msg}");'
                matched = True
                changes += 1
        
        # puts(variable) → can't transform, leave as is but warn
        # (these are dynamic puts, e.g. puts(msg))
        
        # --- printf_shim patterns ---
        # printf_shim("msg\n", 0) → println("msg")  (dummy val=0, no %lld)
        if not matched:
            m = re.match(r'printf_shim\("(.*)\\n",\s*0\);?$', stripped)
            if m:
                fmt = m.group(1)
                if '%' not in fmt:
                    new_line = f'{indent}println("{fmt}");'
                    matched = True
                    changes += 1
                else:
                    # Has format specifiers with val=0 — unusual, leave as println
                    fstr = re.sub(r'%l?l?d', '{0}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}println(f"{fstr}");'
                    matched = True
                    changes += 1
        
        # printf_shim("msg\n", expr) → println(f"msg {expr}")
        if not matched:
            m = re.match(r'printf_shim\("(.*)\\n",\s*(.+)\);?$', stripped)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                if '%' in fmt:
                    fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}println(f"{fstr}");'
                else:
                    new_line = f'{indent}println("{fmt}");'
                matched = True
                changes += 1
        
        # printf_shim("msg", expr) without \n → print(f"...")
        if not matched:
            m = re.match(r'printf_shim\("([^"]*)",\s*(.+)\);?$', stripped)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                if '%' in fmt:
                    fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}print(f"{fstr}");'
                else:
                    new_line = f'{indent}print("{fmt}");'
                matched = True
                changes += 1
        
        # --- print_i64 patterns ---
        # print_i64("msg\n", 0) → println("msg")
        if not matched:
            m = re.match(r'print_i64\("(.*)\\n",\s*0\);?$', stripped)
            if m:
                fmt = m.group(1)
                if '%' not in fmt:
                    new_line = f'{indent}println("{fmt}");'
                else:
                    fstr = re.sub(r'%l?l?d', '{0}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}println(f"{fstr}");'
                matched = True
                changes += 1
        
        # print_i64("msg\n", expr) → println(f"msg {expr}")
        if not matched:
            m = re.match(r'print_i64\("(.*)\\n",\s*(.+)\);?$', stripped)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                if '%' in fmt:
                    fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}println(f"{fstr}");'
                else:
                    new_line = f'{indent}println("{fmt}");'
                matched = True
                changes += 1
        
        # print_i64("msg", expr) without \n → print(f"...")
        if not matched:
            m = re.match(r'print_i64\("([^"]*)",\s*(.+)\);?$', stripped)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                if '%' in fmt:
                    fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}print(f"{fstr}");'
                else:
                    new_line = f'{indent}print("{fmt}");'
                matched = True
                changes += 1
        
        # --- printf("msg\n", expr) → println(f"...") ---
        if not matched:
            m = re.match(r'printf\("(.*)\\n",\s*(.+)\);?$', stripped)
            if m:
                fmt = m.group(1)
                val = m.group(2).strip()
                if '%' in fmt:
                    fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
                    fstr = fstr.replace('%%', '%')
                    new_line = f'{indent}println(f"{fstr}");'
                else:
                    new_line = f'{indent}println("{fmt}");'
                matched = True
                changes += 1
        
        new_lines.append(new_line)
    
    if changes == 0:
        return False, "no changes needed"
    
    result = '\n'.join(new_lines)
    result = re.sub(r'\n{3,}', '\n\n', result)
    
    with open(filepath, 'w') as f:
        f.write(result)
    
    return True, f"{changes} change(s)"


def find_salt_files(root):
    result = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ('target', 'bin', 'node_modules', '.git')]
        for fn in sorted(filenames):
            if fn.endswith('.salt'):
                result.append(os.path.join(dirpath, fn))
    return result

if __name__ == '__main__':
    root = sys.argv[1] if len(sys.argv) > 1 else '.'
    
    dirs_to_scan = [
        os.path.join(root, 'tests'),
        os.path.join(root, 'benchmarks'),
        os.path.join(root, 'examples'),
        os.path.join(root, 'salt-front/tests'),
    ]
    root_files = [os.path.join(root, f) for f in os.listdir(root) 
                  if f.endswith('.salt') and os.path.isfile(os.path.join(root, f))]
    
    all_files = root_files[:]
    for d in dirs_to_scan:
        if os.path.isdir(d):
            all_files.extend(find_salt_files(d))
    
    changed_count = 0
    for fpath in sorted(set(all_files)):
        changed, desc = transform_file(fpath)
        if changed:
            status = "✓"
            rel = os.path.relpath(fpath, root)
            print(f"  {status} {rel}: {desc}")
            changed_count += 1
    
    print(f"\nSecond pass: modernized {changed_count} files.")
