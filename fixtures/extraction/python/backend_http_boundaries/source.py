from fastapi import FastAPI, APIRouter
from flask import Flask, Blueprint
from django.urls import path, re_path, include
import requests
import httpx

app = FastAPI()
router = APIRouter(prefix="/api")

@router.get("/users/{user_id}")
def fastapi_user(user_id: str):
    pass

app.include_router(router, prefix="/v1")

flask_app = Flask(__name__)
bp = Blueprint("users", __name__, url_prefix="/api")

@flask_app.route("/health")
def health():
    pass

@bp.get("/users/<int:user_id>/")
def flask_user(user_id):
    pass

flask_app.register_blueprint(bp, url_prefix="/v1")

urlpatterns = [
    path("healthz"),
    path("users/<int:pk>/", views.detail, name="user-detail"),
    re_path(r"^legacy/(?P<slug>[-\\w]+)/$", views.legacy, name="legacy"),
    path("api/", include("app.urls"), namespace="api"),
]

def call_clients():
    requests.get("https://api.example.com/users")
    httpx.post("/items")

NOTES = '''Bob's notebook: @flask_app.route("/fake-in-string") must stay silent.'''
