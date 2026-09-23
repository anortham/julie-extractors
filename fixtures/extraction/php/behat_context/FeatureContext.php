<?php

use Behat\Behat\Context\Context;
use Behat\Hook\AfterScenario;
use Behat\Hook\BeforeScenario;
use Behat\Step\Given;
use Behat\Step\Then;
use Behat\Step\When;

final class FeatureContext implements Context
{
    private array $cart = [];

    #[Given('an empty cart')]
    public function anEmptyCart(): void
    {
        $this->cart = [];
    }

    #[When('I add :count items')]
    public function iAddItems(int $count): void
    {
        $this->cart = array_fill(0, $count, 'item');
    }

    #[Then('the cart holds :count items')]
    public function theCartHolds(int $count): void
    {
        assert(count($this->cart) === $count);
    }

    #[BeforeScenario]
    public function reset(): void
    {
        $this->cart = [];
    }

    #[AfterScenario]
    public function tidy(): void
    {
        $this->cart = [];
    }

    public function total(): int
    {
        return count($this->cart);
    }
}

class LegacyContext implements Behat\Behat\Context\SnippetAcceptingContext
{
    /**
     * @Given /^a signed-in customer$/
     */
    public function aSignedInCustomer()
    {
    }

    /**
     * @BeforeSuite
     */
    public static function prepare()
    {
    }

    /**
     * @AfterFeature
     */
    public static function cleanup()
    {
    }
}

class WorkflowRules
{
    #[Given('order.placed')]
    public function onOrderPlaced(): void
    {
    }

    /**
     * @Then notify
     */
    public function notify(): void
    {
    }
}
