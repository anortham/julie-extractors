package com.acme.orders;

import java.util.ArrayList;
import java.util.List;
import java.util.function.Supplier;

interface Shape { double area(); }

interface Polygon extends Shape, Comparable<Polygon> { int sides(); }

interface OrderRepository extends JpaRepository<Order, Long>, OrderRepositoryCustom {}

class Base<T> {}

class Repo extends Base<String> implements Shape, java.io.Serializable, Map.Entry<String, Integer> {
    public double area() { return 0; }
}

class Worker extends java.lang.Thread {}

class Login { static class State extends StateBase {} }

class Profile { static class State extends StateBase {} }

public interface Gateway {
    Result charge(long cents);
    default boolean enabled() { return true; }
    static Gateway noop() { return null; }
    record Result(String ref) {}
}

class Point {
    Point(int x) {}
    Point() { this(0); }
}

class Box<T> {
    static Box<String> of() { return new Box<>("x"); }
}

class OrderService extends BaseService {
    private static final Logger LOG = LoggerFactory.getLogger(OrderService.class);
    private final Helper helper = new Helper();

    static { Registry.register("orders"); warmUp(); }

    { helper.prepare(); }

    OrderService(UserRepo repo) {
        super(repo);
        List<String> names = new ArrayList<String>();
        Object index = new java.util.HashMap<String, Integer>();
        Outer.Inner inner = new Outer.Inner();
    }

    static void warmUp() {}

    void run(List<User> users) {
        users.forEach(this::handle);
        users.stream().map(User::getName).forEach(System.out::println);
        Supplier<OrderService> factory = OrderService::new;
    }

    void handle(User user) {}

    int count;

    enum Op {
        ADD(Ops.plus()), SUB(Ops.minus());
        Op(Object f) {}
    }
}
