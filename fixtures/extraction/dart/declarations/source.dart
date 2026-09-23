/// Domain models for the ledger.
library ledger.models;

import 'package:collection/collection.dart' as coll show ListEquality;
import 'src/heavy.dart' deferred as heavy;
import 'dart:convert' hide Codec;

part 'models.g.dart';

/// A strongly typed account identifier.
extension type const AccountId(int value) implements Object {
  AccountId.parse(String text) : this(int.parse(text));

  bool get isValid => value > 0;
}

abstract class Entity {}

abstract class Auditable {}

mixin Tracker on Entity implements Auditable {
  void track() => print('tracked');
}

class Ledger = Entity with Tracker;

enum Mode with Tracker implements Auditable { draft, posted }

/// A money amount in minor units.
class Money {
  final int cents;

  const Money(this.cents);

  const Money.zero() : cents = 0;

  factory Money.redirect(int cents) = Money;

  /// Adds two amounts.
  Money operator +(Money other) => Money(cents + other.cents);

  bool operator ==(Object other) => other is Money && other.cents == cents;

  int operator [](int index) => cents ~/ index;

  int get hashCode => cents.hashCode;

  set rounded(int value) => print(value);
}

class _Cache {
  _Cache._internal();
}

extension on int {
  Money get money => Money(this);
}

typedef void LegacyCallback(String message);

(int, String) pair() => (1, 'a');

Stream<int> ticks() async* {
  yield 1;
}

Iterable<int> counts() sync* {
  yield 2;
}

Future<void> settle(Map<String, Object> json, List<(int, int)> pairs) async {
  await heavy.loadLibrary();
  final (count, label) = pair();
  if (json case {'name': String name, 'age': int age}) {
    print('$name $age $count $label');
  }
  for (final (a, b) in pairs) {
    print(a + b);
  }
  final buffer = StringBuffer()..write('a')..write('b');
  final list = List<int>.filled(3, 0);
  print(buffer.toString() + list.length.toString() + _Cache._internal().toString());
  print(coll.ListEquality<int>().equals([1], [1]));
}
