using Reqnroll;
using TechTalk.SpecFlow;

namespace Shop.Specs;

[Binding]
public class CartSteps
{
    private readonly Cart cart = new();

    [Given(@"an empty cart")]
    public void GivenAnEmptyCart() => cart.Clear();

    [When(@"I add (\d+) items")]
    public void WhenIAddItems(int count) => cart.Add(count);

    [Then(@"the cart holds (\d+) items")]
    public void ThenTheCartHolds(int count) => Assert.Equal(count, cart.Count);

    [StepDefinition(@"the cart is saved")]
    public void TheCartIsSaved() => cart.Save();

    [BeforeScenario]
    public void Reset() => cart.Clear();

    [AfterScenario]
    public void Tidy() => cart.Save();

    public int Total() => cart.Count;
}

[TechTalk.SpecFlow.Binding]
public sealed class CheckoutSteps
{
    [Reqnroll.Given("a signed-in customer")]
    public void GivenASignedInCustomer() { }
}

public class WorkflowRules
{
    [When("order.placed")]
    public void OnOrderPlaced() { }

    [Then("notify")]
    public void Notify() { }
}
