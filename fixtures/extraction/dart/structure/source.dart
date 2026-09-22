/// Default request timeout.
const kTimeout = Duration(seconds: 30);
final logger = Logger('app');
/// The signed-in user, if any.
String? currentUser;

/// A shopping cart.
class Cart {
  /// Items in the cart.
  final List<Item> items = [];
  static const maxItems = 50;
  final controller = TextEditingController();
  int count = 0, limit = 10;

  Cart(this.store) : retries = defaultRetries() {
    store.connect();
  }

  const Cart.empty() : store = null;

  factory Cart.create() {
    return Cart(Store());
  }

  final Store store;

  int get size {
    return items.length;
  }

  set size(int value) {
    resize(value);
  }

  User? get owner => lookupOwner();

  int total();

  void add(Item item) {
    items.add(item);
    this.recalc();
    recalc();
    Cart.create();
  }

  void recalc() {}

  void wire(Button button, List<Map<String, dynamic>> rows) {
    button.onPressed = () => recalc();
    final users = rows.map((row) => User.fromJson(row)).toList();
    final spacing = const Spacing(8);
    final other = new Cart.empty();
  }
}

class Item {}

class Report {
  void run() {
    format();
    upload();
  }
}

class Export {
  void run() {
    format();
    upload();
  }
}

void format() {}
