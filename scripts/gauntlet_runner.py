import subprocess
import time
import os
import json
import statistics
import re

class Metrics:
    def __init__(self, compilation_ms, execution_avg_ns, cycles, loc):
        self.compilation_ms = compilation_ms
        self.execution_avg_ns = execution_avg_ns
        self.cycles = cycles
        self.loc = loc

def run_cmd(cmd, silence=False):
    if silence:
        cmd += " 2>/dev/null"
    return subprocess.check_output(cmd, shell=True).decode()

def count_loc(path):
    try:
        with open(path) as f:
            return sum(1 for line in f if line.strip() and not line.strip().startswith("//") and not line.strip().startswith("#"))
    except:
        return 0

def benchmark_generic(path, build_cmd, run_cmd_str, iterations=50):
    name = os.path.basename(path)
    print(f"[{name}]", end=" ", flush=True)
    
    # LOC
    loc = count_loc(path)
    
    # Compilation
    start = time.time()
    if build_cmd:
        try:
            run_cmd(build_cmd, silence=True)
        except Exception as e:
            print(f"Build Failed: {e}")
            return Metrics(0, 0, 0, loc)
    comp_ms = (time.time() - start) * 1000
    
    # Execution
    exec_times = []
    
    # Warmup
    try:
        run_cmd(run_cmd_str, silence=True)
    except:
        return Metrics(comp_ms, 0, 0, loc)
        
    for i in range(iterations):
        start_exec = time.time_ns()
        try:
            run_cmd(run_cmd_str, silence=True)
            exec_times.append(time.time_ns() - start_exec)
        except:
            break
        if i % 10 == 0: print(".", end="", flush=True)
    
    print(" Done.")
    avg_ns = int(statistics.mean(exec_times)) if exec_times else 0
    return Metrics(compilation_ms=comp_ms, execution_avg_ns=avg_ns, cycles=0, loc=loc)

def benchmark_salt(path, iterations=50):
    name = os.path.basename(path).replace(".salt", "")
    mlir_path = f"benchmarks/leetcode/{name}.mlir"
    ll_path = f"benchmarks/leetcode/{name}.ll"
    bin_path = f"benchmarks/leetcode/{name}_salt"
    
    print(f"[{name}.salt]", end=" ", flush=True)
    loc = count_loc(path)
    
    start = time.time()
    try:
        # Pass --release to suppress debug traces
        run_cmd(f"./salt-front/target/debug/salt-front {path} --release > {mlir_path}", silence=True)
        run_cmd(f"./salt/build/salt-opt --verify --emit-llvm {mlir_path} --output {ll_path}", silence=True)
        run_cmd(f"clang {ll_path} benchmarks/bridge.c -o {bin_path} -O3", silence=True)
    except Exception as e:
        print(f"Build Failed: {e}")
        return Metrics(0, 0, 0, loc)
    comp_ms = (time.time() - start) * 1000
    
    exec_times = []
    cycles_list = []
    
    run_cmd(bin_path, silence=True) # Warmup
    
    for i in range(iterations):
        start_exec = time.time_ns()
        output = run_cmd(bin_path, silence=False) # Capture output for Cycles
        exec_times.append(time.time_ns() - start_exec)
        
        match = re.search(r"Cycles: (\d+)", output)
        if match: cycles_list.append(int(match.group(1)))
        
        if i % 10 == 0: print(".", end="", flush=True)
        
    print(" Done.")
    avg_ns = int(statistics.mean(exec_times)) if exec_times else 0
    avg_cycles = int(statistics.mean(cycles_list)) if cycles_list else 0
    
    return Metrics(compilation_ms=comp_ms, execution_avg_ns=avg_ns, cycles=avg_cycles, loc=loc)

if __name__ == "__main__":
    print("--- 🏁 Comparative Algorithmic Gauntlet ---")
    results = {}
    
    benchmarks = [
        "sudoku_solver",
        "reverse_list",
        "trie",
        "merge_sorted_lists",
        "binary_tree_path",
        "lru_cache",
        "trapping_rain_water"
    ]
    
    for b in benchmarks:
        print(f"\n--- Benchmark: {b} ---")
        results[b] = {}
        
        # 1. Salt
        results[b]["Salt"] = benchmark_salt(f"benchmarks/leetcode/{b}.salt")
        
        # 2. C
        results[b]["C"] = benchmark_generic(
            f"benchmarks/leetcode/{b}.c",
            f"gcc benchmarks/leetcode/{b}.c -o benchmarks/leetcode/{b}_c -O3",
            f"./benchmarks/leetcode/{b}_c"
        )
        
        # 3. C++
        results[b]["C++"] = benchmark_generic(
            f"benchmarks/leetcode/{b}.cpp",
            f"g++ benchmarks/leetcode/{b}.cpp -o benchmarks/leetcode/{b}_cpp -O3",
            f"./benchmarks/leetcode/{b}_cpp"
        )
        
        # 4. Rust
        results[b]["Rust"] = benchmark_generic(
            f"benchmarks/leetcode/{b}.rs",
            f"rustc benchmarks/leetcode/{b}.rs -o benchmarks/leetcode/{b}_rs -O",
            f"./benchmarks/leetcode/{b}_rs"
        )
        
        # 5. Python
        results[b]["Python"] = benchmark_generic(
            f"benchmarks/leetcode/{b}.py",
            None,
            f"python3 benchmarks/leetcode/{b}.py"
        )

    print("\n" + "="*100)
    print(f"{'Benchmark':<20} | {'Lang':<8} | {'LOC':<5} | {'Comp (ms)':<10} | {'Exec (ns)':<12} | {'Rel Speed':<10}")
    print("-" * 100)
    
    for b in benchmarks:
        base_ns = results[b]["C"].execution_avg_ns
        for lang in ["Salt", "C", "C++", "Rust", "Python"]:
            m = results[b][lang]
            rel = f"{m.execution_avg_ns / base_ns:.2f}x" if base_ns > 0 and m.execution_avg_ns > 0 else "N/A"
            print(f"{b:<20} | {lang:<8} | {m.loc:<5} | {m.compilation_ms:<10.2f} | {m.execution_avg_ns:<12} | {rel:<10}")
        print("-" * 100)
    print("="*100)
