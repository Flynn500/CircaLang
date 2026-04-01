# Circa

A programming language where precision is a first-class concept. Every numeric value can carry a tolerance, and that tolerance propagates through arithmetic and function calls automatically. Write your computation once, then dial precision up or down to trade accuracy for speed.

## Quick Start

```bash
circa --run main.ca
```

## The Basics

Values in Circa can carry a tolerance with the `~` operator:

```
let x = 3.14 ~ 0.01      // x is 3.14, known to ±0.01
let y = 9.81               // y is exact (tolerance = 0)
```

Tolerance propagates through arithmetic automatically:

```
let a = 10.0 ~ 0.1
let b = 20.0 ~ 0.2

print(a + b)    // 30 ~ 0.3
print(a + 5.0)  // 15 ~ 0.1   (exact values don't add uncertainty)
print(a * b)    // 200 ~ 3.0  (product rule: |a|*tol(b) + |b|*tol(a))
```

## Functions

Regular functions work as expected. Tolerance flows through them via normal arithmetic:

```
fn kinetic_energy(mass, velocity) {
    return 0.5 * mass * velocity * velocity
}

let m = 2.0 ~ 0.1
let v = 3.0 ~ 0.05

let ke = kinetic_energy(m, v)
print(ke)   // 9.0 ~ 0.75
```

No special syntax needed, the tolerance on `m` and `v` propagates through the multiplication automatically.

```
fn distance(x1, y1, x2, y2) {
    let dx = x2 - x1
    let dy = y2 - y1
    
    //sqrt is a tolerance-aware function from the standard library. its result is gauranteed 
    //to be within 0.001 of the true result, accounting for the uncertainty of its input params.
    //The function will try and do as little as possible to meet these requirements.
    return sqrt(dx * dx + dy * dy) ~tol 0.001
}

let d = distance(0.0, 0.0, 3.0 ~= 0.1, 4.0 ~= 0.1)
print(d)    // 5.0 ~= ...
```

## Tolerance-Aware Functions (`~tol`)

Some functions can accept a precision target with `~tol_variable`. This value functions similarly to a standard variable, with the caveat that the tolerance of the return value is automatically set to this value. The goal of a tolerance aware function is to do as little work as possible to achieve a result within `tol`.

```
fn solve(f, a, b) ~tol {
    // Brent's method, iterates until |f(b)| <= tol
    // ...
    return b    // return value automatically carries ~= tol
}

// Find sqrt(2) to different precisions
let rough = solve(fn(x) { x*x - 2.0 }, 0.0, 2.0) ~tol 0.1
print(rough)    // 1.4190476 ~= 0.1

let exact = solve(fn(x) { x*x - 2.0 }, 0.0, 2.0) ~tol 0.001
print(exact)    // 1.4140716 ~= 0.001
```

The `~tol` parameter does two things: it controls how hard the function works internally, and the return value is automatically tagged with that tolerance. Same code, different precision, different compute cost.

If a function can't meet the requested tolerance because the input values are too uncertain, it panics:

```
let noisy = 1.0 ~ 0.5
let y = sin(noisy) ~tol 0.01   // panic: input uncertainty exceeds requested tol
```

## Standard Library

The standard library provides tolerance-aware implementations of common math functions. Each one adapts its algorithm (Taylor series terms, Newton iterations, etc.) to do the minimum work needed to meet the requested `~tol`:

```
let x = 1.0

print(sqrt(2.0) ~tol 0.0001)        // 1.4142135 ~= 0.0001
print(sin(x) ~tol 0.01)             // 0.8333333 ~= 0.01
print(cos(x) ~tol 0.001)            // 0.5403023 ~= 0.001
print(exp(x) ~tol 0.0001)           // 2.718254 ~= 0.0001
print(ln(2.7182818) ~tol 0.0001)    // 1.0000262 ~= 0.0001
```

Looser tolerance = fewer iterations = faster results. The `sin(1.0) ~tol 0.01` call above only computes two Taylor terms because that's all it needs.

We also include statistical functions, numerical methods (solve, intergrate etc.), linear algebra methods for matrices & vectors. See docs/std

## Why?

Most numerical code computes everything to machine precision whether you need it or not. Circa makes precision explicit:

- Rough exploratory calculation? Use `~tol 0.1` and get answers fast.
- Final results for a report? Tighten to `~tol 0.00001`.
- Same code, same functions, different precision budget.

Tolerance on values tells you what you actually know. A sensor reading of `9.81 ~= 0.05` is more honest than a bare `9.81`, and Circa tracks that honesty through every operation.