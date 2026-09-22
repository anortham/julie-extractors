import httpx
import requests
from django.urls import include, path, re_path
from fastapi import APIRouter
from flask import Flask
from requests import get
from rest_framework import routers, viewsets
from rest_framework.decorators import action, api_view

from . import views

app: Flask = Flask(__name__)
typed_router: APIRouter = APIRouter(prefix="/t")
router = routers.DefaultRouter()
router.register(r"users", views.UserViewSet, basename="user")


@app.route("/health")
def health():
    return "ok"


@typed_router.get("/typed")
def typed():
    pass


class UserViewSet(viewsets.ModelViewSet):
    @action(detail=True, methods=["post"], url_path="set-password")
    def set_password(self, request, pk=None):
        pass

    @action(detail=False)
    def recent(self, request):
        pass


@api_view(["GET", "POST"])
def status(request):
    pass


async def fetch_users():
    async with httpx.AsyncClient(base_url="https://api.example.com") as client:
        return await client.get("/users")


def sync_session():
    session = requests.Session()
    session.post("https://api.example.com/login")
    get("https://api.example.com/plain")
    requests.post(url="https://api.example.com/c")


def unproven(session):
    session.post("https://unproven.example.com")


urlpatterns = [
    re_path(r"^blog/(page-(\d+)/)?$", views.blog),
    re_path(r"^users/(\d+)/$", views.user),
    re_path(r"^articles/(?P<year>[0-9]{4})/(?P<month>[0-9]{2})/?$", views.month),
    re_path(r"^feed\.xml$", views.feed),
    re_path(r"^(rss|atom)/$", views.feeds),
    path("api/v1/", include(("api.urls", "api"), namespace="v1")),
    path("api/", include(router.urls)),
]
