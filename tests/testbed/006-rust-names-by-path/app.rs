mod outer {
    pub mod inner {
        pub struct Widget;

        impl Widget {
            // No `self`: an associated function, the same fact `singleton`
            // carries for a Ruby `def self.x`.
            pub fn new() -> Widget {
                Widget
            }

            pub fn run(&self, count: u8) -> u8 {
                count + 1
            }
        }

        impl std::fmt::Debug for Widget {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Widget")
            }
        }
    }
}

trait Loud {
    // A bodyless signature: a real name with nothing to hash.
    fn shout(&self) -> String;

    fn whisper(&self) -> String {
        String::new()
    }
}

fn main() {}
