#!/usr/bin/env ruby
# frozen_string_literal: true
#
# App Store Connect from the command line, with nothing but Ruby's
# standard library: the API wants an ES256 JWT, and OpenSSL can make one.
#
# Credentials come from the environment, never from this file:
#   ASC_KEY_ID      the key's id, ten characters, e.g. ABCD1234EF
#   ASC_ISSUER_ID   the team's issuer id (a UUID). Omit for an
#                   individual key, which is scoped to one person instead.
#   ASC_KEY         path to AuthKey_<id>.p8, if it is not in the usual
#                   ~/.appstoreconnect/private_keys/
#
# Usage:
#   tools/asc.rb apps                     # what the key can see
#   tools/asc.rb certs                    # signing certificates
#   tools/asc.rb bundle-ids
#   tools/asc.rb get /v1/apps limit=5
#   tools/asc.rb post /v1/certificates '<json>'
require 'base64'
require 'json'
require 'net/http'
require 'openssl'
require 'uri'

HOST = 'api.appstoreconnect.apple.com'

def die(msg)
  warn "error: #{msg}"
  exit 1
end

def key_id
  ENV['ASC_KEY_ID'] || die('set ASC_KEY_ID (the 10-character key id)')
end

def key_path
  explicit = ENV['ASC_KEY']
  return explicit if explicit && File.exist?(explicit)

  guess = File.expand_path("~/.appstoreconnect/private_keys/AuthKey_#{key_id}.p8")
  return guess if File.exist?(guess)

  die("no private key. Put AuthKey_#{key_id}.p8 in " \
      '~/.appstoreconnect/private_keys/ or set ASC_KEY to its path')
end

def b64(data)
  Base64.urlsafe_encode64(data).delete('=')
end

# ES256 signatures are a DER sequence of two integers on the wire, but
# JOSE wants them as a bare 64-byte r||s pair.
def jose_signature(der)
  r, s = OpenSSL::ASN1.decode(der).value.map(&:value)
  [r, s].map { |n| [n.to_s(16).rjust(64, '0')].pack('H*') }.join
end

def token
  header = { alg: 'ES256', kid: key_id, typ: 'JWT' }
  now = Time.now.to_i
  payload = { aud: 'appstoreconnect-v1', iat: now, exp: now + 600 }
  # A team key names the team that issued it; an individual key speaks
  # for one person and says so instead.
  if (issuer = ENV['ASC_ISSUER_ID'])
    payload[:iss] = issuer
  else
    payload[:sub] = 'user'
  end

  signing_input = "#{b64(JSON.dump(header))}.#{b64(JSON.dump(payload))}"
  key = OpenSSL::PKey::EC.new(File.read(key_path))
  der = key.sign(OpenSSL::Digest.new('SHA256'), signing_input)
  "#{signing_input}.#{b64(jose_signature(der))}"
end

def request(method, path, body: nil, query: nil)
  query = nil if query.nil? || query.empty?
  uri = URI::HTTPS.build(host: HOST, path: path, query: query)
  klass = { 'GET' => Net::HTTP::Get, 'POST' => Net::HTTP::Post,
            'PATCH' => Net::HTTP::Patch, 'DELETE' => Net::HTTP::Delete }.fetch(method)
  req = klass.new(uri)
  req['Authorization'] = "Bearer #{token}"
  req['Content-Type'] = 'application/json'
  req.body = body if body

  res = Net::HTTP.start(uri.host, uri.port, use_ssl: true) { |http| http.request(req) }
  parsed = res.body.to_s.empty? ? {} : JSON.parse(res.body)

  unless res.code.start_with?('2')
    errors = (parsed['errors'] || []).map do |e|
      [e['title'], e['detail']].compact.join(': ')
    end
    die("#{method} #{path} -> HTTP #{res.code}\n  #{errors.join("\n  ")}")
  end
  parsed
end

def rows(data, *fields)
  Array(data['data']).each do |item|
    values = fields.map do |f|
      f == 'id' ? item['id'] : item.dig('attributes', f)
    end
    puts values.join('  ')
  end
  puts "(#{Array(data['data']).size} shown)"
end

command = ARGV.shift

case command
when 'token'
  puts token

when 'get'
  path = ARGV.shift || die('usage: get <path> [query]')
  puts JSON.pretty_generate(request('GET', path, query: ARGV.join('&')))

when 'post'
  path = ARGV.shift || die('usage: post <path> <json>')
  puts JSON.pretty_generate(request('POST', path, body: ARGV.shift))

when 'patch'
  path = ARGV.shift || die('usage: patch <path> <json>')
  puts JSON.pretty_generate(request('PATCH', path, body: ARGV.shift))

when 'apps'
  rows(request('GET', '/v1/apps', query: 'limit=200'), 'id', 'bundleId', 'name', 'sku')

when 'bundle-ids'
  rows(request('GET', '/v1/bundleIds', query: 'limit=200'), 'id', 'identifier', 'name', 'platform')

when 'certs'
  rows(request('GET', '/v1/certificates', query: 'limit=200'),
       'id', 'certificateType', 'displayName', 'expirationDate')

when 'profiles'
  rows(request('GET', '/v1/profiles', query: 'limit=200'),
       'id', 'profileType', 'name', 'profileState')

when 'key-json'
  # fastlane's cert, sigh and deliver all take the key this way.
  out = ARGV.shift || 'target/asc-key.json'
  issuer = ENV['ASC_ISSUER_ID'] || die('set ASC_ISSUER_ID to write a fastlane key file')
  File.write(out, JSON.pretty_generate(
    key_id: key_id, issuer_id: issuer, key: File.read(key_path), in_house: false
  ))
  File.chmod(0o600, out)
  puts "wrote #{out} (contains the private key -- keep it out of git)"

else
  puts File.read(__FILE__).lines[2..26].map { |l| l.sub(/^#\s?/, '') }.join
  exit(command.nil? ? 0 : 1)
end
