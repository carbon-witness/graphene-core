/*
 * Copyright (c) 2026 Graphene contributors.
 *
 * The MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#pragma once

#include <fc/filesystem.hpp>
#include <fc/network/tcp_socket.hpp>
#include <fc/optional.hpp>
#include <fc/thread/mutex.hpp>

#include <memory>
#include <string>

// OpenSSL types, so that this header does not pull in the OpenSSL headers
typedef struct ssl_ctx_st SSL_CTX;
typedef struct ssl_st SSL;
typedef struct bio_st BIO;

namespace graphene { namespace net {

/**
 * TLS for P2P connections.
 *
 * TLS goes under the existing stcp encryption: a TLS connection carries the same stcp key
 * exchange and messages as a plain one, so nothing above stcp_socket changes, and the
 * signature in the hello message still authenticates the peer. TLS only makes the traffic
 * look like an ordinary TLS connection instead of random bytes, which some networks with
 * DPI let through when they cut the plain protocol. Certificates are not verified.
 *
 * Both protocols share the listening port: a TLS client starts with a ClientHello (first
 * byte 0x16), a plain stcp client with a compressed public key (0x02 or 0x03).
 */
enum class p2p_tls_mode
{
   off,       ///< plain connections only, the node behaves as before TLS support
   automatic, ///< accept both; try TLS on outbound connections, fall back to plain for old peers
   always     ///< accept both; outbound connections use TLS only
};

p2p_tls_mode p2p_tls_mode_from_string( const std::string& mode );
std::string to_string( p2p_tls_mode mode );

struct p2p_tls_options
{
   p2p_tls_mode            mode = p2p_tls_mode::automatic;
   /// Certificate (chain) and private key in PEM for the server side; without them the node
   /// generates a self-signed certificate at startup
   fc::optional<fc::path>  certificate_file;
   fc::optional<fc::path>  private_key_file;
};

/// OpenSSL contexts shared by all the P2P connections of a node
class tls_context
{
public:
   explicit tls_context( const p2p_tls_options& options );
   ~tls_context();
   tls_context( const tls_context& ) = delete;
   tls_context& operator=( const tls_context& ) = delete;

   p2p_tls_mode mode() const { return _mode; }
   SSL_CTX*     client_context() const { return _client; }
   SSL_CTX*     server_context() const { return _server; }

private:
   p2p_tls_mode _mode;
   SSL_CTX*     _client = nullptr;
   SSL_CTX*     _server = nullptr;
};

/// The first byte of a TLS record carrying a handshake message (ClientHello, ServerHello)
constexpr char tls_handshake_record_type = 0x16;
/// The first byte of a TLS alert record
constexpr char tls_alert_record_type = 0x15;

/**
 * TLS over an fc::tcp_socket through OpenSSL memory BIOs.
 *
 * The encrypted bytes go through the tcp_socket itself, so its I/O hooks (the P2P rate
 * limiter and its byte counters) keep working, and everything runs in the fibers of the
 * calling thread like the plain socket. One fiber may read while another writes.
 */
class tls_connection
{
public:
   enum class role { client, server };

   /// @param server_name SNI for a client; empty for none (an IP address must not be sent as SNI)
   tls_connection( const tls_context& context, fc::tcp_socket& socket, role r,
                   const std::string& server_name = std::string() );
   ~tls_connection();
   tls_connection( const tls_connection& ) = delete;
   tls_connection& operator=( const tls_connection& ) = delete;

   /// Bytes already read from the socket, such as the first byte read to detect the protocol
   void feed( const char* data, size_t length );

   /// Completes the TLS handshake. A client throws p2p_tls_not_supported when the server
   /// answers with something other than TLS (an old node sends its stcp key right away).
   void handshake();

   /// Reads at least one byte of plaintext; throws fc::eof_exception when the peer closed
   size_t readsome( char* buffer, size_t length );
   /// Writes all of @p length bytes of plaintext
   void write( const char* buffer, size_t length );

   /// The negotiated protocol version, such as "TLSv1.3"
   std::string version() const;

private:
   void flush_output();
   void read_input();

   SSL*                  _ssl = nullptr;
   BIO*                  _input = nullptr;  ///< network -> OpenSSL, owned by _ssl
   BIO*                  _output = nullptr; ///< OpenSSL -> network, owned by _ssl
   fc::tcp_socket&       _socket;
   role                  _role;
   bool                  _received_anything = false;
   std::shared_ptr<char> _input_buffer;
   std::shared_ptr<char> _output_buffer;
   fc::mutex             _output_mutex;
};

} } // graphene::net
