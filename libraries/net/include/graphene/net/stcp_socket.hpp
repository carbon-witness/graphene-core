/*
 * Copyright (c) 2015 Cryptonomex, Inc., and contributors.
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
#include <fc/network/tcp_socket.hpp>
#include <fc/crypto/aes.hpp>
#include <fc/crypto/elliptic.hpp>

#include <graphene/net/tls.hpp>

#include <memory>
#include <string>
#include <vector>

namespace graphene { namespace net {

/**
 *  Uses ECDH to negotiate a aes key for communicating
 *  with other nodes on the network.
 */
class stcp_socket : public virtual fc::iostream
{
  public:
    stcp_socket();
    ~stcp_socket();
    fc::tcp_socket&  get_socket() { return _sock; }

    /**
     * TLS settings for the next accept() or connect_to(); without a context (or with TLS off)
     * the socket speaks the plain protocol only.
     * @param outbound_tls connect_to() starts TLS; otherwise it connects plain
     * @param server_name  SNI for an outbound TLS connection, empty for none
     */
    void             set_tls( std::shared_ptr<const tls_context> context, bool outbound_tls,
                              const std::string& server_name = std::string() );
    /// Whether the connection runs over TLS (known after accept() or connect_to())
    bool             is_tls() const { return bool(_tls); }
    /// The negotiated TLS version, empty for a plain connection
    std::string      tls_version() const { return _tls ? _tls->version() : std::string(); }

    /// Accepts the plain protocol, and TLS when the TLS context allows it: the first byte the
    /// client sends tells them apart
    void             accept();

    void             connect_to( const fc::ip::endpoint& remote_endpoint );
    void             bind( const fc::ip::endpoint& local_endpoint );

    virtual size_t   readsome( char* buffer, size_t max );
    virtual size_t   readsome( const std::shared_ptr<char>& buf, size_t len, size_t offset );
    virtual bool     eof()const;

    virtual size_t   writesome( const char* buffer, size_t len );
    virtual size_t   writesome( const std::shared_ptr<const char>& buf, size_t len, size_t offset );

    virtual void     flush();
    virtual void     close();

    using istream::get;
    void             get( char& c ) { read( &c, 1 ); }
    fc::sha512       get_shared_secret() const { return _shared_secret; }
  private:
    void do_key_exchange();

    // The connection under the stcp encryption: TLS, or the TCP socket itself
    size_t raw_readsome( const std::shared_ptr<char>& buffer, size_t length, size_t offset );
    void   raw_read( const std::shared_ptr<char>& buffer, size_t length, size_t offset = 0 );
    void   raw_write( const std::shared_ptr<const char>& buffer, size_t length );

    std::shared_ptr<const tls_context> _tls_context;
    bool                               _outbound_tls = false;
    std::string                        _tls_server_name;
    std::unique_ptr<tls_connection>    _tls;
    /// Bytes read from a plain client to detect the protocol, served before the socket
    std::vector<char>                  _unread_input;

    fc::sha512           _shared_secret;
    fc::ecc::private_key _priv_key;
    fc::tcp_socket       _sock;
    fc::aes_encoder      _send_aes;
    fc::aes_decoder      _recv_aes;
    std::shared_ptr<char> _read_buffer;
    std::shared_ptr<char> _write_buffer;
#ifndef NDEBUG
    bool _read_buffer_in_use;
    bool _write_buffer_in_use;
#endif
};

typedef std::shared_ptr<stcp_socket> stcp_socket_ptr;

} } // graphene::net
