require_relative "lib/aruaru/db/version"

Gem::Specification.new do |spec|
  spec.name          = "aruaru-db"
  spec.version       = Aruaru::Db::VERSION
  spec.authors       = ["aon-co-jp"]
  spec.summary       = "aruaru-db official Ruby connector (thin wrapper over the pg gem)"
  spec.description   = <<~DESC
    Thin wrapper over the standard `pg` gem adding aruaru-db's Git-on-SQL
    helpers (#commit / #query_as_of). Not a custom driver -- see
    docs/CLIENTS.md in the aruaru-db repository. Works with Rails
    (ActiveRecord's postgresql adapter) or plain PG::Connection.
  DESC
  spec.homepage      = "https://github.com/aon-co-jp/aruaru-db"
  spec.license       = "MIT"
  spec.required_ruby_version = ">= 3.0"

  spec.files         = Dir["lib/**/*.rb"] + ["README.md"]
  spec.require_paths = ["lib"]

  spec.add_dependency "pg", ">= 1.4", "< 2.0"

  spec.add_development_dependency "rspec", "~> 3.13"
end
