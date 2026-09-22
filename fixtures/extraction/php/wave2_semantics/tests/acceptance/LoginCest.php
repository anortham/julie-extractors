<?php
class LoginCest
{
    public function _before(AcceptanceTester $I): void { $I->amOnPage('/login'); }
    public function loginWorks(AcceptanceTester $I): void { $I->see('Dashboard'); }
    public function _after(AcceptanceTester $I): void {}
    protected function fillForm(AcceptanceTester $I): void {}
}
