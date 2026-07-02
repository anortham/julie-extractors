import express from "express";
import fastify from "fastify";

export function routes() {
  const app = express();
  const router = express.Router();

  app.use("/api", router);
  router.get("/users/:id", (_req, res) => res.send("ok"));
  app.route("/reports/:reportId").get((_req, res) => res.send("report"));

  const server = fastify();
  server.route({ method: ["GET", "POST"], url: "/fast/:id", handler: async () => ({ ok: true }) });
}
