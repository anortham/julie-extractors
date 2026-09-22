#include "remote_base.h"

namespace company::product::detail {
struct Deep { int x; };
void deep_fn() {}
}

namespace net {
/// Handler invoked per request.
using Handler = std::function<void(int)>;
template <typename T>
using List = std::vector<T>;
}

class ENGINE_API Repo : public RemoteBase, public ns::Mixin {
public:
    Repo();
    ~Repo();
    std::optional<int> find(int id) const;
    static std::unique_ptr<Repo> make(double r);
    bool operator==(const Repo& other) const;
    operator bool() const;
    using Ptr = Repo*;
    static int instances;
private:
    int compute() const;
    const std::string& name_;
    int buffer_[16];
    void (*callback_)(int);
};

Repo::Repo() {}
Repo::~Repo() { compute(); }
int Repo::compute() const { return 1; }
bool Repo::operator==(const Repo& other) const { return true; }
Repo::operator bool() const { return true; }
int Repo::instances = 0;

class Outer { class Inner; };
class Outer::Inner : public Repo { public: void work(); };
template <> struct hash<Repo> { size_t operator()(const Repo& r) const noexcept; };

Repo* g_default = nullptr;
Repo* g_current = g_default;
std::string g_first, g_second;
void (*g_handler)(int) = nullptr;

void save(Repo* other, const std::string& path) {
    Repo& alias = *other;
    auto [key, value] = split(path);
    std::lock_guard<std::mutex> lock(mutex_);
    std::ofstream out(path);
    Writer writer(out, alias);
    writer.flush();
}
