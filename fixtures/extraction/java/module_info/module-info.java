/** Shop module. */
module com.acme.shop {
    requires java.sql;
    requires transitive com.acme.core;
    exports com.acme.shop.api;
    exports com.acme.shop.internal to com.acme.web;
    opens com.acme.shop.model;
    uses com.acme.shop.spi.PaymentProvider;
    provides com.acme.shop.spi.PaymentProvider with com.acme.shop.impl.StripeProvider;
}
