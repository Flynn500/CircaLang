# Circa

A programming language where precision is a first-class concept. Every numeric value can carry a tolerance, and that tolerance propagates through arithmetic and function calls automatically. Write your computation once, then dial precision up or down to trade accuracy for speed.

## Quick Start

```bash
circa --run main.ca
```

## The Basics

Values in Circa can carry a tolerance with the `~` operator:

```rust
let x = 3.14 ~ 0.01      // x is 3.14, known to ±0.01
let y = 9.81               // y is exact (tolerance = 0)
```

Tolerance propagates through arithmetic automatically:

```rust
let a = 10.0 ~ 0.1
let b = 20.0 ~ 0.2

print(a + b)    // 30 ~ 0.3
print(a + 5.0)  // 15 ~ 0.1   (exact values don't add uncertainty)
print(a * b)    // 200 ~ 4.0  (product rule: |a|*tol(b) + |b|*tol(a))
```

We can compare values in Circa using standard comparison operators `<`, `==` etc. The expression `a > b` will return true if `a` is gauranteed to be greater than `b`. Circa also includes the addition `?` operator, which can be paired with any of the standard comparison operators and will evaluate to true if the expression could be true given the tolerence of inputs.

```rust
let a = 1 ~ 0.5
let b = 1.2 ~ 0.5

//this will return true, a could equal b
print(a ?= b)

//both of these could be true, so both return true
print(a ?> b)
print(b ?> a)

//a and b are not gauranteed to be equal
print(a == b)

let c = 1 ~ 0.1
let d = 1 ~ 0.1

//false, even though these values identical, they are not gauranteed to be the same
print(c == d)
```

## Types

Circa currently supports the following primitive types:

- Integer (i64)
- Float (f64)
- String

## Functions

Regular functions work as expected. Tolerance flows through them via normal arithmetic:

```rust
fn kinetic_energy(mass, velocity) {
    return 0.5 * mass * velocity * velocity
}

let m = 2.0 ~ 0.1
let v = 3.0 ~ 0.05

let ke = kinetic_energy(m, v)
print(ke)   // 9.0 ~ 0.75
```

No special syntax needed, the tolerance on `m` and `v` propagates through the multiplication automatically.

```rust
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

Circa also supports lambas, these can be declared in the same way functions with the name ommited.

```rust
import math

//passing a lamda into our solve function
let root = solve(fn(x) { return x * x - 2.0 }, 0.0, 5.0)
```


## Tolerance-Aware Functions (`~tol`)

Some functions can accept a precision target with `~tol_variable`. This value functions similarly to a standard variable, with the caveat that the tolerance of the return value is automatically set to this value. The goal of a tolerance aware function is to do as little work as possible to achieve a result within `tol`.

```rust
//an example of a simple tolerence aware function that estimates the value of pi. 
fn estimate_pi() ~tol {
    let n = 1.0 / tol
    let sum = 0.0
    let i = 0
    let sign = 1.0

    loop {
        sum = sum + sign / (2.0 * i + 1.0)
        sign = sign * -1.0
        i = i + 1
        if i >= n { break }
    }

    return sum * 4.0
}
```

The `~tol` parameter does two things: it controls how hard the function works internally, and the return value is automatically tagged with that tolerance. Same code, different precision, different compute cost.

If a function can't meet the requested tolerance because the input values are too uncertain, it panics:

```rust
let noisy = 1.0 ~ 0.5
let y = sin(noisy) ~tol 0.01   // panic: input uncertainty exceeds requested tol
```

## Loops

Currently Circa only supports `loop` & `break` however while and for loops can be easily emulated.
```rust
let fib_target = 10

let a = 0
let b = 1
let i = 0

loop {
    if i > fib_target { break }

    let temp = a + b
    a = b
    b = temp

    i = i + 1
}
print(b)
```
## Structs

We can define a struct using the `struct` keyword. Variables are declared using the let keyword, while struct methods are declared be defining functions within the struct body. Struct methods require a `self` parameter.

```rust
struct Point {
    let x
    let y

    fn magnitude(self)~tol {
        return sqrt(self.x * self.x + self.y * self.y) ~tol
    }

    fn add(self, other) {
        return new Point { x = self.x + other.x, y = self.y + other.y }
    }
}
```

We create structs using the `new` keyword. Struct methods and variables support tolerence in the same way functions and standard variables do.

```rust
let noisy = new Point { x = 3.0 ~ 0.1, y = 4.0 ~ 0.1 }
print(noisy.magnitude() ~ 0.05)
```

## Vectors & Matrices

We can define vectors using the let keyword. Like all variables, vector elements can also carry tolerence. 

```rust
let v = [1.0 ~ 0.1, 2.0, 3.0]
let x = v[0]

//we can push elements to vectors
v.push(4 ~ 0.1)

//and extend a vector using other vectors
let v2 = [5,6,7]
v.extend(v2)
```

Matrices are still in development.

## Modules

A module in Circa is just a file. Using the `import` keyword, you can import Circa files in the same directory as the main file, or modules from the standard library. Circula imports aren't an issue in Circa, but function names and constants must be unique across multiple modules in a project. 

```rust
import foo

print(bar(50))
```

## Standard Library

The standard library provides tolerance-aware implementations of common math functions. Each one adapts its algorithm (Taylor series terms, Newton iterations, etc.) to do the minimum work needed to meet the requested `~tol`:

```rust
import math

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

Tolerance on values tells you what you actually know. A sensor reading of `9.81 ~= 0.05` is more honest than a bare `9.81`, and Circa tracks that honesty through every operation. You can pipe together a series of operations, apporixmating where applicable and the resulting value is gauranteed to be within its tolerence threshold of the true result.