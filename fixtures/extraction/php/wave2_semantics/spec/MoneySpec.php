<?php
namespace spec\App\Billing;

use PhpSpec\ObjectBehavior;

class MoneySpec extends ObjectBehavior
{
    function let() { $this->beConstructedWith(100); }
    function it_is_initializable() { $this->shouldHaveType(Money::class); }
    function its_cents_are_positive() { $this->cents()->shouldBe(100); }
    function helper() {}
}
