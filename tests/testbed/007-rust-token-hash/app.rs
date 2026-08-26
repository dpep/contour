struct Alpha;
struct Beta;
struct Gamma;
struct Delta;

impl Alpha {
    fn total(items: &[u8], start: u8) -> u8 {
        let mut sum = start;
        for item in items {
            sum += item;
        }
        sum
    }
}

impl Beta {
    // Reformatted and re-commented. The token tier sees through both, and the
    // function's own name is excluded, so this collides with Alpha::total.
    fn accumulate(items: &[u8], start: u8) -> u8 {
        let mut sum = start;
        for item in items { sum += item; }   // squashed onto one line
        sum
    }
}

impl Gamma {
    // Renamed locals. Ruby's normalization would see through this; the Rust
    // token tier does NOT, and that is the honest limit of `token_hash`.
    fn total(values: &[u8], base: u8) -> u8 {
        let mut acc = base;
        for value in values {
            acc += value;
        }
        acc
    }
}

impl Delta {
    // Genuinely different: subtraction, not addition.
    fn total(items: &[u8], start: u8) -> u8 {
        let mut sum = start;
        for item in items {
            sum -= item;
        }
        sum
    }
}
