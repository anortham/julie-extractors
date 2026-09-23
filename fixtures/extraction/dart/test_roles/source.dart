import 'package:bloc_test/bloc_test.dart';
import 'package:test/test.dart';

void main() {
  group('calculator', () {
    setUp(() {});

    test('adds two numbers', () {
      expect(2 + 2, equals(4));
    });

    blocTest<CounterCubit, int>(
      'emits one after increment',
      build: () => CounterCubit(),
      act: (cubit) => cubit.increment(),
      expect: () => [1],
    );

    runExample('ordinary callback', () {});
  });
}

void testNamedButNotCalled() {}

void runExample(String name, void Function() callback) {
  callback();
}
