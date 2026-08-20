class ManagedTestRoles extends AnyFunSpec with BeforeAndAfterEach {
  describe("scala roles") {
    it("extracts a Scala test case") {
    }
  }

  def ordinaryCase(): Unit = ()

  override def beforeEach(): Unit = ()

  def callOrdinaryMember(): Unit = {
    feature.enable("ordinary member call") {
      ()
    }
  }
}
