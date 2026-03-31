import time

def count_loop(n):
    i = 0.0
    acc = 0.0
    while i < n:
        acc = acc + i * 0.5
        i = i + 1.0
    return acc

def add_one(x):
    return x + 1.0

def call_chain(n):
    x = 0.0
    i = 0.0
    while i < n:
        x = add_one(x)
        i = i + 1.0
    return x

def tol_arithmetic(n):
    # No tolerance in python, just mirror the arithmetic
    x = 1.0
    i = 0.0
    while i < n:
        x = x + x
        x = x * 1.0
        i = i + 1.0
    return x

def approx_id(x):
    return x

def tol_call_loop(n):
    i = 0.0
    while i < n:
        r = approx_id(1.0)
        i = i + 1.0
    return i

def vec_work(n):
    i = 0.0
    acc = 0.0
    while i < n:
        v = [1.0, 2.0, 3.0, 4.0, 5.0]
        acc = acc + v[0] + v[4]
        i = i + 1.0
    return acc

N = 10000.0

start = time.perf_counter()

print(count_loop(N))
print(call_chain(N))
print(tol_arithmetic(N))
print(tol_call_loop(N))
print(vec_work(N))

elapsed = (time.perf_counter() - start) * 1000
print(f"Execution time: {elapsed:.4f}ms")