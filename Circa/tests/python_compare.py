import time
start = time.time()

def fib(n):
    if n <= 1:
        return n

    a = 0
    b = 1

    for _ in range(2, n + 1):
        a, b = b, a + b

    return b

print(fib(50))

end = time.time()
print("Execution time:", (end - start)*1000)