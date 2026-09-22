require "application_system_test_case"

class UsersTest < ApplicationSystemTestCase
  setup do
    @user = users(:one)
  end

  test "visiting the index" do
    visit users_url
  end
end

class UserMailerTest < ActionMailer::TestCase
  test "welcome" do
    assert_emails 1
  end
end

class CleanupJobTest < ActiveJob::TestCase
  def test_enqueues
    assert_enqueued_jobs 0
  end
end
