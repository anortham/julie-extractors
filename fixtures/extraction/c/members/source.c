#include <assert.h>
_Static_assert(sizeof(long) >= 4, "long too small");
#include "module.h"
MODULE_LICENSE("GPL");

/** Connection states. */
enum State {
    STATE_IDLE,
    STATE_OPEN, /**< Connected and ready. */
};

struct Ops { void (*run)(int); };

/*! A tagged value with an anonymous union. */
struct __attribute__((packed)) Value {
    int tag; /**< Which member is live. */
    /// Source position.
    struct {
        int line, col;
    } pos;
    union {
        long as_int;
        double as_float;
    };
    unsigned short len __attribute__((aligned(2)));
};

struct Server { struct Ops *ops; void (*on_request)(int code); };

[[maybe_unused]] static int debug_level = 0;
__attribute__((section(".data.hot"))) int hot_counter;

//! Formats a log line.
int fmt_log(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
int api_version(void);
int api_count(void);
enum State current_state(void);
static struct Value *find_value(int id);
union Pun { int i; float f; };
union Pun make_pun(void) { union Pun p; p.i = 0; return p; }

////////////////////////////////////////
long value_int(struct Value *v) {
    static_assert(sizeof(int) == 4, "int size");
    return v->as_int + v->pos.line + _Generic(v->tag, int: 1, default: 0);
}

void serve(struct Server *srv) {
    srv->on_request(200);
    srv->ops->run(1);
    (*srv->on_request)(201);
    MODULE_MARKER;
}
