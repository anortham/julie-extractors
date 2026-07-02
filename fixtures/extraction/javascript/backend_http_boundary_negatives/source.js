module.exports = function (app) {
  app.get("/health", handler);
  app.get("port");
};
