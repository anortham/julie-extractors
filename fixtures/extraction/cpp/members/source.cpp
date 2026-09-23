#include <vector>
#include "engine/renderer.h"
#define APP_VERSION "1.2.0"
#define CLAMP(v, lo, hi) ((v) < (lo) ? (lo) : (v))

//! Qt-style line doc for the app namespace.
namespace app {

struct Point {
    std::string label;
    bool operator==(const Point& o) const { return compare(label, o.label); }
};

/*! A connection that closes its handle. */
class [[deprecated("use Channel")]] Conn {
public:
    ~Conn() { close_handle(fd); }
    Conn& operator<<(const std::string& s) { write_all(fd, s); return *this; }
    explicit operator bool() const { return fd >= 0; }
    template <typename U> void push(U&& value) {}
    template <typename U> U* get() const;
    friend std::ostream& operator<<(std::ostream& os, const Conn& c);
    [[nodiscard]] int value() const;
    virtual void start() = 0;
    Conn() = default;
    int fd; ///< The open descriptor.
    /// Retries before giving up.
    int retries = default_retries();
};

enum class Mode { Fast, ///< Skip validation.
    Safe };

class Allocator { public: void* allocate(int n); };

template <typename Allocator, typename Iterator>
void fill(Iterator first, Iterator last, Allocator& alloc) { alloc.allocate(1); }

Point* find_point(int id);
auto trailing() -> Point { return {}; }
std::vector<std::shared_ptr<Point>> all_points() { return {}; }
void label(ns::Widget w);

}

namespace {
void anon_fn() {}
}
static int g_count = 0;

////////////////////////////////////////
int range_for(const std::vector<int>& v) {
    int s = 0;
    for (int x : v) { s += x; }
    try { if (s > 0) return CLAMP(s, 0, 10); } catch (const std::exception& e) { return 2; } catch (...) { return 3; }
    return s;
}

template <typename... Args> void log_all(const char* fmt, Args&&... args) {}
