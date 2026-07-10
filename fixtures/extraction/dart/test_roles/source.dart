import 'package:test/test.dart';

void main() {
  group('calculator', () {
    setUp(() {});

    test('adds two numbers', () {
      expect(2 + 2, equals(4));
    });

    runExample('ordinary callback', () {});
  });
}

void testNamedButNotCalled() {}

void runExample(String name, void Function() callback) {
  callback();
}
