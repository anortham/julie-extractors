import 'package:dio/dio.dart';
import 'package:go_router/go_router.dart';
import 'package:http/http.dart' as http;
import 'package:shelf_router/shelf_router.dart';

final router = GoRouter(routes: [
  GoRoute(
    path: '/users/:id',
    name: 'user',
    builder: (context, state) => const UserPage(),
    routes: [
      GoRoute(path: 'posts', builder: (context, state) => PostsPage()),
    ],
  ),
]);

void openUser(BuildContext context) {
  context.go('/users/42');
  context.pushNamed('user');
}

class UserApi {
  @Route.get('/users/<id>')
  Response fetch(Request request, String id) => Response.ok(id);

  @Route('PATCH', '/users/<id>')
  Response rename(Request request, String id) => Response.ok(id);
}

final app = Router()..get('/health', health);

Response health(Request request) => Response.ok('ok');

void mount() {
  app.post('/items', createItem);
}

class RemoteSource {
  RemoteSource(this._dio);

  final Dio _dio;

  Future<void> sync() async {
    await http.get(Uri.parse('https://api.example.com/v1/users'));
    await http.post(Uri.https('api.example.com', '/v1/users'));
    await _dio.get<Map<String, Object>>('/v1/orders');
    await _dio.delete('/v1/orders/7');
  }
}
