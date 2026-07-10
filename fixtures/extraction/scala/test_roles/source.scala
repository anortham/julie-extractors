class ManagedTestRoles extends AnyFunSpec {
  describe("scala roles") {
    it("extracts a Scala test case") {
    }
  }

  def ordinaryCase(): Unit = ()

  def callOrdinaryMember(): Unit = {
    feature.enable("ordinary member call") {
      ()
    }
  }
}
