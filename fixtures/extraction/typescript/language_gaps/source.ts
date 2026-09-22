import express, { Router } from "express";
import fastify from "fastify";
import { Entity } from "./entity";
import { Logger } from "./logger";
import { UsersService } from "./users.service";
import * as api from "./api";

/** Shape base. */
export abstract class Shape extends Entity implements Drawable {
  /** Area of the shape. */
  abstract area(): number;
  protected abstract readonly kind: string;
  describe(): string {
    return `${this.kind}: ${this.area()}`;
  }
}

export class Circle extends Shape {
  protected readonly kind = "circle";
  area(): number {
    return 3;
  }
}

interface Drawable {
  draw(): void;
}

class Repository<T> {
  find(id: string): T | undefined {
    return undefined;
  }
}

interface User {
  id: string;
}

class UserRepo extends Repository<User> {
  constructor(private readonly logger: Logger) {
    super();
  }

  load(id: string): void {
    super.find(id);
    this.findById(id);
    this.logger.warn("load");
    const audit: Logger = this.logger;
    audit.info(id);
    this.touch();
  }

  touch(): void {}
}

enum Color {
  /** Red. */
  Red,
  Green,
  Blue = 5,
  "quoted-key" = 6,
}

export enum Status {
  Active = "active",
  Disabled = "disabled",
}

/** Plain const doc. */
const retries = 3;

/** Arrow doc. */
const double = (value: number) => value * 2;

/** Exported function doc. */
export function run(repo: UserRepo, log: Logger): number {
  const svc = new UsersService();
  svc.findAll();
  api.fetchUser(retries);
  repo.load("1");
  log.warn("run");
  return double(retries);
}

export class Local {
  findAll(): void {}
  fetchUser(): void {}
}

export const app = express();
app.get("/healthz", (_req, res) => res.json({ ok: true }));

const routes: Router = express.Router();
routes.get("/users", (_req, res) => res.json([]));

const items = express.Router();
items
  .route("/:id")
  .get((_req, res) => res.json(1))
  .delete((_req, res) => res.sendStatus(204));

export const server = fastify({ logger: true });
server.get("/ping", async () => "pong");
