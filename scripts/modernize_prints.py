#!/usr/bin/env python3
"""
Salt Codebase Modernization: Replace print externs with println/f-strings.
Transforms:
  puts("msg\n\0")        → println("msg")
  puts("msg\0")          → print("msg")
  printf_shim("fmt %lld\n\0", val)  → println(f"fmt {val}")
  print_i64("fmt %lld\0", val)      → print(f"fmt {val}")
  
Also removes the extern declarations for puts, printf_shim, print_i64, printf.
Preserves extern fn malloc and other non-print externs.
"""
import re
import sys
import os

# Files/dirs to skip
SKIP_FILES = {
    'option.salt',   # stdlib panic path
    'result.salt',   # stdlib panic path
}

def transform_file(filepath):
    """Transform a single .salt file. Returns (changed, description)."""
    basename = os.path.basename(filepath)
    if basename in SKIP_FILES:
        return False, "skipped (stdlib)"
    
    with open(filepath, 'r') as f:
        content = f.read()
    
    original = content
    lines = content.split('\n')
    new_lines = []
    removed_externs = []
    changes = 0
    
    for line in lines:
        stripped = line.strip()
        
        # Remove print-related extern declarations
        if re.match(r'^extern fn (puts|printf_shim|print_i64)\b', stripped):
            removed_externs.append(stripped)
            changes += 1
            continue
        # Also handle printf (but not printf_shim since already caught above)
        if re.match(r'^extern fn printf\b', stripped) and 'printf_shim' not in stripped:
            removed_externs.append(stripped)
            changes += 1
            continue
        
        # Transform puts("...\n\0") → println("...")
        # Pattern: puts("text\n\0"); or puts("text\n\0")
        m = re.match(r'^(\s*)puts\("(.*)\\n\\0"\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            text = m.group(1).strip()  # leading whitespace from stripped
            msg = m.group(2)
            trail = m.group(3)
            # Re-extract indent from original line
            indent = re.match(r'^(\s*)', line).group(1)
            new_lines.append(f'{indent}println("{msg}");{trail}')
            changes += 1
            continue
        
        # Transform puts("...\0") → print("...") (no trailing newline)
        m = re.match(r'^(\s*)puts\("(.*)\\0"\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            msg = m.group(2)
            trail = m.group(3)
            new_lines.append(f'{indent}print("{msg}");{trail}')
            changes += 1
            continue
            
        # Transform printf_shim("fmt %lld\n\0", val) → println(f"fmt {val}")
        m = re.match(r'^(\s*)printf_shim\("(.*)\\n\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            # Replace %lld, %d, %ld with {val}
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            # Replace %% with %
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}println(f"{fstr}");{trail}')
            changes += 1
            continue
        
        # Transform printf_shim("fmt %lld\0", val) → print(f"fmt {val}") (no newline)
        m = re.match(r'^(\s*)printf_shim\("(.*)\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}print(f"{fstr}");{trail}')
            changes += 1
            continue
        
        # Transform print_i64("fmt %lld\n\0", val) → println(f"fmt {val}")
        m = re.match(r'^(\s*)print_i64\("(.*)\\n\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}println(f"{fstr}");{trail}')
            changes += 1
            continue
        
        # print_i64 without \n
        m = re.match(r'^(\s*)print_i64\("(.*)\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}print(f"{fstr}");{trail}')
            changes += 1
            continue
        
        # Transform printf("fmt\0", val) → println/print (same patterns)
        m = re.match(r'^(\s*)printf\("(.*)\\n\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}println(f"{fstr}");{trail}')
            changes += 1
            continue
        
        m = re.match(r'^(\s*)printf\("(.*)\\0",\s*(.+)\);?(.*)$', stripped)
        if m:
            indent = re.match(r'^(\s*)', line).group(1)
            fmt = m.group(2)
            val = m.group(3).strip().rstrip(';')
            trail = m.group(4)
            fstr = re.sub(r'%l?l?d', '{' + val + '}', fmt)
            fstr = fstr.replace('%%', '%')
            new_lines.append(f'{indent}print(f"{fstr}");{trail}')
            changes += 1
            continue
        
        new_lines.append(line)
    
    if changes == 0:
        return False, "no changes needed"
    
    # Clean up consecutive blank lines (from removed externs)
    result = '\n'.join(new_lines)
    result = re.sub(r'\n{3,}', '\n\n', result)
    
    with open(filepath, 'w') as f:
        f.write(result)
    
    return True, f"{changes} change(s), removed: {', '.join(removed_externs) if removed_externs else 'none'}"

def find_salt_files(root):
    """Find all .salt files recursively."""
    result = []
    for dirpath, dirnames, filenames in os.walk(root):
        # Skip build dirs
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
    # Also check root-level stray files
    root_files = [os.path.join(root, f) for f in os.listdir(root) 
                  if f.endswith('.salt') and os.path.isfile(os.path.join(root, f))]
    
    all_files = root_files[:]
    for d in dirs_to_scan:
        if os.path.isdir(d):
            all_files.extend(find_salt_files(d))
    
    changed_count = 0
    for fpath in sorted(set(all_files)):
        changed, desc = transform_file(fpath)
        status = "✓" if changed else "·"
        rel = os.path.relpath(fpath, root)
        print(f"  {status} {rel}: {desc}")
        if changed:
            changed_count += 1
    
    print(f"\nModernized {changed_count} files.")
