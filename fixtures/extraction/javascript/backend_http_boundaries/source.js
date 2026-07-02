import express from "express";
import fastify from "fastify";

export function routes() {
  const app = express();
  const router = express.Router();
  app.use("/api", router);
  router.get("/users/:id", showUser);
  app.route("/items/:id").get(showItem).post(updateItem);

  const server = fastify();
  server.route({ method: ["GET", "POST"], url: "/fast/:id", handler });
}
