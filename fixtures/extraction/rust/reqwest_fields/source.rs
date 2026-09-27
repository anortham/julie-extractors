use reqwest::Client;

struct Service {
    client: reqwest::Client,
    bare_client: Client,
}

struct NonHttp {
    client: String,
}

fn local_service() {
    struct Service {
        client: String,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/local-owner").await;
        }
    }
}

mod imported {
    use reqwest::Client;

    struct Service {
        client: Client,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/imported").await;
        }
    }
}

mod local {
    struct Client;

    struct Service {
        client: Client,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/local").await;
        }
    }
}

mod other {
    pub struct Service {
        pub client: String,
    }
}

impl Service {
    async fn load(&self, other: NonHttp) {
        self.client.get("https://example.com/qualified").await;
        self.bare_client.post("https://example.com/bare").await;
        other.client.get("https://example.com/other").await;
    }
}

impl NonHttp {
    async fn load(&self) {
        self.client.get("https://example.com/non-http").await;
    }
}

impl other::Service {
    async fn load(&self) {
        self.client.get("https://example.com/qualified-owner").await;
    }
}
