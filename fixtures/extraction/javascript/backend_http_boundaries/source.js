import express from "express";
import fastify from "fastify";

export function routes() {
  const app = express();
  const router = express.Router();
  app.use("/api", router);
  router.get("/users/:id", showUser);
  app.route("/items/:id").get(showItem).post(updateItem);

  app.get("port");
  app.route("/multi/:id")
    .get(readMulti)
    .post(writeMulti);
  app.route("/cache").get((req, res) => res.json(cache.get(key)));

  const server = fastify();
  server.route({ method: ["GET", "POST"], url: "/fast/:id", handler });
}
