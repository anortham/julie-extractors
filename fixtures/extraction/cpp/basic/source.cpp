/// Base identity provider.
class Base {
public:
    int id() const {
        return 1;
    }
};

/// Worker pipeline implementation.
class Worker : public Base {
public:
    explicit Worker(int id) : id_(id) {}

    /// Run the worker helper pipeline.
    [[nodiscard]]
    int run() const {
        log("worker-run");
        this->helper(id_);
        return helper(id_);
    }

    void ping() const;

private:
    int helper(int value) const {
        return value + 1;
    }

    int id_;
};

void Worker::ping() const {
    this->id();
    (*this).id();
}

/// Convert a raw value into a helper result.
[[nodiscard]]
int helper_value(int value) {
    return value + 2;
}

int run_worker() {
    return helper_value(20);
}

int evaluate(int count, bool enabled) {
    int total = 0;
    if (enabled) {
        for (int i = 0; i < count; i++) {
            total += i;
        }
    }
    return total;
}

template<typename K, typename V>
struct Map {};

template<typename T>
struct Vec {};

struct Item {};

Map<int, Vec<Item>> worker_index;

void use_facts(const Item& a, Item* b, Item c, Item&& d) {
    auto made = std::make_unique<Item>();
    auto unknown = Unknown();
    Item declared;
    auto constructed = Item();
    auto allocated = new Item();
}
