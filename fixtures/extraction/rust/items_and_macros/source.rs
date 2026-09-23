//! Items, impl members, and item macros.

use std::cell::RefCell;
pub use std::sync::Mutex;

macro_rules! twice {
    ($e:expr) => {
        $e * 2
    };
}

/// A shape with a unit.
#[allow(unused)]
pub trait Shape {
    /// The unit of measure.
    type Unit;
    /// Area in units.
    fn area(&self) -> f64;
    fn scaled(&self, by: f64) -> f64 {
        self.area() * by
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Square {
    #[doc(hidden)]
    pub side: f64,
}

impl Square {
    pub const MAX: u32 = 10;

    /// Parses input.
    // NOTE: plain comments do not break rustdoc attachment.
    pub fn outer(&self) -> u32 {
        fn inner_helper(x: u32) -> u32 {
            twice!(x)
        }
        struct Tmp {
            v: u32,
        }
        let t = Tmp { v: 1 };
        inner_helper(t.v)
    }

    pub fn me() -> Self {
        Square { side: 1.0 }
    }
}

impl Shape for Square {
    type Unit = f64;
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

#[serde(tag = "kind")]
enum Event {
    #[serde(rename = "created")]
    Created { id: u32 },
    Closed,
}

#[deprecated(note = "use MAX")]
pub const OLD: u32 = 1;
pub static GLOBAL: Square = Square { side: 2.0 };

lazy_static! {
    /// Global registry.
    pub static ref REGISTRY: Mutex<Vec<u32>> = Mutex::new(vec![]);
}

thread_local! {
    static CACHE: RefCell<u32> = RefCell::new(0);
}

bitflags! {
    pub struct Flags: u32 {
        const A = 1;
        const B = 2;
    }
}

pub fn code(n: i32) -> &'static str {
    match n {
        0 => "zero",
        x if x < 0 => "neg",
        _ => "many",
    }
}

pub fn guarded(a: Option<u32>) -> u32 {
    let Some(w) = a else { return 0 };
    tracing::info!("value {}", w);
    REGISTRY.lock().unwrap().push(w);
    w
}

pub mod a {
    pub struct Config;
    impl Config {
        pub fn load() -> Self {
            Config
        }
    }
}

pub mod b {
    pub struct Config;
    impl Config {
        pub fn load() -> Self {
            Config
        }
    }
}
