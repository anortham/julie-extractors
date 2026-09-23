const express = require("express");
const { Router } = require("express");
const { describe, it, afterEach } = require("mocha");
const after = require("after");
const { validate, audit: log } = require("./validate");
const Resolver = require("./resolver").Resolver;
const usersRouter = require("./routes/users");
import * as db from "./db";
import { fetchPage } from "./api";

/** Add two numbers. @returns {number} */
export const add = (a, b) => a + b;
const mul = function (a, b) {
  return a * b;
};

/** Service class. */
export class UserService {
  handle = async (event) => event;
}

/** Proto method. */
Thing.prototype.legacy = function legacy(x) {
  return fetchPage(x);
};

/** Member assigned. */
exports.helper = function helper() {
  return db.find(1);
};

export function* paginate() {
  yield fetchPage(1);
}

export const loader = function () {
  return fetchPage(2);
};

function wrap() {
  (function iife() {
    fetchPage(3);
  })();
}

async function getUser(id) {
  const user = await db.user(id);
  return user;
}

class Loader {
  resolve(name) {
    return name;
  }
}

function useHook(state, { label, onClick: handler }, [first]) {
  const { toggle, data: payload, meta: { total }, ...rest } = state;
  toggle();
  validate(payload);
  log(total);
  return new Resolver(label, handler, first, rest, add(1, 2), mul(2, 3));
}

describe("counter", function () {
  afterEach(function () {});
  it("waits for both", function (done) {
    const cb = after(2, done);
    fetchPage(4);
    cb();
  });
});

const app = express();
const admin = Router();
app.use("/users", usersRouter);
app.use("/v2", require("./routes/v2"));
app.use("/admin", requireAuth, admin);
admin.get("/stats", (req, res) => res.json({}));
const books = express.Router();
books
  .route("/books/:id")
  .get((req, res) => res.json({}));

function Queue() { this.clear(); }
Queue.prototype.clear = function clear() { this.jobs = []; };
Queue.prototype.drain = function drain() { this.clear(); };
Queue.create = () => new Queue();

/**
 * @param {UserRepo} repo
 * @returns {Promise<User>}
 */
async function loadUser(repo) { return repo.find(1); }

class Service {
  /** @type {Cache} */
  cache = null;
  @observable items = [];
  constructor(store) { this.store = store; this.logger = new Logger(); }
}

const Widget = class extends Service { render() { return this.logger; } };
const settings = { port: 3000, nested: { deep: 1 }, start() {} };
track("load", { fetchUser: true });

function a() {} function b() {}
export { a, b as bee };
export * from "./all.js";
export * as ns from "./ns.js";
export let m = 1, n = 2;
export function* ids() {}

const KoaRouter = require("@koa/router");
const Hapi = require("@hapi/hapi");
const koa = new KoaRouter({ prefix: "/v1" });
koa.get("/orders/:id", async (ctx) => { ctx.body = {}; });
const hapiServer = Hapi.server({ port: 3000 });
hapiServer.route({ method: "GET", path: "/items/{id}", handler: (r) => r.params.id });
