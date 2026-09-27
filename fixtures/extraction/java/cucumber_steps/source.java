package examples;

import io.cucumber.java.en.And;
import io.cucumber.java.en.But;
import io.cucumber.java.en.Given;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

class CheckoutSteps {
    @Given("there are cucumbers")
    void has_cucumbers() {}

    @When("the application starts")
    void starts_application() {}

    @Then("the application is ready")
    void application_is_ready() {}

    @And("the application is open")
    void application_is_open() {}

    @But("the application is not closed")
    void application_is_not_closed() {}

    void helper() {}
}

class QualifiedSteps {
    @io.cucumber.java.en.Given("the qualified application starts")
    void qualified_step() {}
}
