import express from "express";
import fastify from "fastify";

export function routes(): void {
  const app = express();
  const router = express.Router();

  app.use("/api", router);
  router.get("/users/:id", (_req, res) => res.send("ok"));
  app.route("/reports/:reportId").get((_req, res) => res.send("report"));

  const server = fastify();
  server.route({ method: ["GET", "POST"], url: "/fast/:id", handler: async () => ({ ok: true }) });
}

import Router from "@koa/router";
import Hapi from "@hapi/hapi";

export function koaAndHapiRoutes(): void {
  const koa = new Router({ prefix: "/v1" });
  koa.post("/orders", async (ctx) => { ctx.body = {}; });
  const hapi = Hapi.server({ port: 3000 });
  hapi.route({ method: "DELETE", path: "/items/{id}", handler: () => null });
}
