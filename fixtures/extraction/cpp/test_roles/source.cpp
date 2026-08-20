TEST_CASE("vector grows", "[vector]") {
    SECTION("push grows") {
        REQUIRE(true);
    }
}

void helper_named_like_a_test_case() {}

class GoogleFixture : public ::testing::Test {
protected:
    void SetUp() override {}
    void TearDown() override {}
    static void SetUpTestSuite() {}
    static void TearDownTestSuite() {}
};

class ParameterizedFixture : public ::testing::TestWithParam<int> {
protected:
    void SetUp() override {}
    void TearDown() override {}
    static void SetUpTestSuite() {}
    static void TearDownTestSuite() {}
};

class IndirectFixture : public GoogleFixture {
protected:
    void SetUp() override {}
    void TearDown() override {}
    static void SetUpTestSuite() {}
    static void TearDownTestSuite() {}
};

class UnrelatedFixture : public OtherBase {
protected:
    void SetUp() {}
    void TearDown() {}
    static void SetUpTestSuite() {}
    static void TearDownTestSuite() {}
};

class OutOfClassFixture : public ::testing::Test {
public:
    void SetUp() override;
    void TearDown() override;
    static void SetUpTestSuite();
    static void TearDownTestSuite();
};

class OutOfClassParameterizedFixture : public ::testing::TestWithParam<int> {
public:
    void SetUp() override;
    void TearDown() override;
    static void SetUpTestSuite();
    static void TearDownTestSuite();
};

class OutOfClassIndirectFixture : public OutOfClassFixture {
public:
    void SetUp();
};

class OutOfClassUnrelatedFixture : public OtherBase {
public:
    void SetUp();
};

void OutOfClassFixture::SetUp() {}
void OutOfClassFixture::TearDown() {}
void OutOfClassFixture::SetUpTestSuite() {}
void OutOfClassFixture::TearDownTestSuite() {}
void OutOfClassParameterizedFixture::SetUp() {}
void OutOfClassParameterizedFixture::TearDown() {}
void OutOfClassParameterizedFixture::SetUpTestSuite() {}
void OutOfClassParameterizedFixture::TearDownTestSuite() {}
void OutOfClassIndirectFixture::SetUp() {}
void OutOfClassUnrelatedFixture::SetUp() {}

void SetUp() {}
