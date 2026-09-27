class Alpha {
    static int helper() {
        return 1;
    }

    int caller() {
        return helper();
    }

    int choose() {
        return 2;
    }

    int choose(int value) {
        return value;
    }

    int overload_caller() {
        return choose();
    }
}

class Beta {
    static int helper() {
        return 3;
    }
}

class Owner {
    private static class Hidden {
    }

    Object local() {
        return new Hidden();
    }
}

class Outsider {
    Object foreign() {
        return new Hidden();
    }
}
