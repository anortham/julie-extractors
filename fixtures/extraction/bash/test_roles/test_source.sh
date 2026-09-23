Describe 'bash roles'
  Context 'addition'
    It 'extracts a ShellSpec test case'
    End
  End
End

test_named_case() {
  return 0
}

setup() {
  return 0
}

calculate_total() {
  printf '%s\n' 2
}

It.helper 'ordinary dotted command'

Describe 'hooks'
  BeforeEach 'setup'
  AfterAll cleanup_roles
  xIt 'skipped case'
    When call calculate_total
  End
  fDescribe 'focused'
    fIt 'focused case'
    End
  End
End

testCamelCase() {
  return 0
}

oneTimeSetUp() {
  return 0
}

oneTimeTearDown() {
  return 0
}
