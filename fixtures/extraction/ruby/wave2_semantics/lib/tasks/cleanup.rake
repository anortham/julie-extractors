namespace :cleanup do
  desc "Remove stale users"
  task stale_users: :environment do
    CleanupJob.perform_later(30)
  end
end
