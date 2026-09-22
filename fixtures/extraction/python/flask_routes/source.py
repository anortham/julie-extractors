import flask
from app.main import bp

app = flask.Flask(__name__)


@bp.route("/index", methods=["GET", "POST"])
def index():
    return "index"


@bp.get("/user/<username>")
def user(username):
    return username


@app.route("/health")
def health():
    return "ok"


def ping():
    return "pong"


app.add_url_rule("/ping", view_func=ping, methods=["POST"])
