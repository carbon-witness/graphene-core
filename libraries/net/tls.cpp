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
#include <graphene/net/tls.hpp>
#include <graphene/net/exceptions.hpp>

#include <fc/crypto/hex.hpp>
#include <fc/crypto/rand.hpp>
#include <fc/exception/exception.hpp>
#include <fc/log/logger.hpp>
#include <fc/thread/scoped_lock.hpp>

#include <openssl/bio.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/pem.h>
#include <openssl/ssl.h>
#include <openssl/x509.h>

#include <algorithm>
#include <memory>

namespace graphene { namespace net {

namespace {

constexpr size_t tls_io_buffer_size = 16 * 1024;

/// The errors OpenSSL queued for this thread, oldest first; clears the queue
std::string openssl_errors()
{
   std::string result;
   while( unsigned long code = ERR_get_error() )
   {
      char text[256];
      ERR_error_string_n( code, text, sizeof(text) );
      if( !result.empty() )
         result += "; ";
      result += text;
   }
   return result.empty() ? std::string("no details") : result;
}

[[noreturn]] void throw_tls_error( const std::string& what )
{
   FC_THROW_EXCEPTION( p2p_tls_error, "${what}: ${details}", ("what", what)("details", openssl_errors()) );
}

/// A self-signed EC P-256 certificate for the server side of P2P connections without a
/// configured one. Peers do not verify it; the hello message authenticates the node.
void use_self_signed_certificate( SSL_CTX* context )
{
   std::unique_ptr<EVP_PKEY, decltype(&EVP_PKEY_free)> key( EVP_EC_gen("P-256"), &EVP_PKEY_free );
   if( !key )
      throw_tls_error( "generating the key of the P2P TLS certificate" );
   std::unique_ptr<X509, decltype(&X509_free)> certificate( X509_new(), &X509_free );
   if( !certificate )
      throw_tls_error( "creating the P2P TLS certificate" );

   char serial[16];
   fc::rand_bytes( serial, sizeof(serial) );
   serial[0] &= 0x7f; // a positive serial number
   std::unique_ptr<BIGNUM, decltype(&BN_free)> serial_number(
         BN_bin2bn( reinterpret_cast<const unsigned char*>(serial), sizeof(serial), nullptr ), &BN_free );
   // Something like a host name, not an obvious marker of the software
   const std::string common_name = fc::to_hex( serial + 8, 4 ) + ".local";

   X509_NAME* name = X509_get_subject_name( certificate.get() );
   if( !serial_number
       || !X509_set_version( certificate.get(), 2 )
       || !BN_to_ASN1_INTEGER( serial_number.get(), X509_get_serialNumber( certificate.get() ) )
       || !X509_gmtime_adj( X509_getm_notBefore( certificate.get() ), -24 * 3600 )
       || !X509_gmtime_adj( X509_getm_notAfter( certificate.get() ), 365L * 24 * 3600 )
       || !X509_set_pubkey( certificate.get(), key.get() )
       || !X509_NAME_add_entry_by_txt( name, "CN", MBSTRING_ASC,
                                       reinterpret_cast<const unsigned char*>(common_name.c_str()), -1, -1, 0 )
       || !X509_set_issuer_name( certificate.get(), name )
       || !X509_sign( certificate.get(), key.get(), EVP_sha256() ) )
      throw_tls_error( "creating the P2P TLS certificate" );

   if( SSL_CTX_use_certificate( context, certificate.get() ) != 1
       || SSL_CTX_use_PrivateKey( context, key.get() ) != 1 )
      throw_tls_error( "loading the P2P TLS certificate" );
}

SSL_CTX* new_context( const SSL_METHOD* method )
{
   SSL_CTX* context = SSL_CTX_new( method );
   if( !context )
      throw_tls_error( "creating a TLS context" );
   SSL_CTX_set_min_proto_version( context, TLS1_2_VERSION );
   // Renegotiation would need reads inside writes; TLS 1.3 has none, and TLS 1.2 does not need it
   SSL_CTX_set_options( context, SSL_OP_NO_RENEGOTIATION );
   SSL_CTX_set_verify( context, SSL_VERIFY_NONE, nullptr );
   return context;
}

} // anonymous namespace

p2p_tls_mode p2p_tls_mode_from_string( const std::string& mode )
{
   if( mode == "off" )
      return p2p_tls_mode::off;
   if( mode == "auto" )
      return p2p_tls_mode::automatic;
   if( mode == "always" )
      return p2p_tls_mode::always;
   FC_THROW_EXCEPTION( fc::invalid_arg_exception,
                       "Unknown P2P TLS mode \"${mode}\", expected auto, always or off", ("mode", mode) );
}

std::string to_string( p2p_tls_mode mode )
{
   switch( mode )
   {
      case p2p_tls_mode::off:    return "off";
      case p2p_tls_mode::always: return "always";
      default:                   return "auto";
   }
}

tls_context::tls_context( const p2p_tls_options& options ) :
   _mode( options.mode )
{
   if( _mode == p2p_tls_mode::off )
      return;
   try
   {
      _client = new_context( TLS_client_method() );
      _server = new_context( TLS_server_method() );
      if( options.certificate_file || options.private_key_file )
      {
         FC_ASSERT( options.certificate_file && options.private_key_file,
                    "The P2P TLS certificate needs both the certificate and the private key file" );
         if( SSL_CTX_use_certificate_chain_file( _server, options.certificate_file->string().c_str() ) != 1 )
            throw_tls_error( "loading the P2P TLS certificate " + options.certificate_file->string() );
         if( SSL_CTX_use_PrivateKey_file( _server, options.private_key_file->string().c_str(), SSL_FILETYPE_PEM ) != 1 )
            throw_tls_error( "loading the P2P TLS private key " + options.private_key_file->string() );
         if( SSL_CTX_check_private_key( _server ) != 1 )
            throw_tls_error( "the P2P TLS private key does not match the certificate" );
      }
      else
         use_self_signed_certificate( _server );
   }
   catch( ... )
   {
      SSL_CTX_free( _client );
      SSL_CTX_free( _server );
      throw;
   }
}

tls_context::~tls_context()
{
   SSL_CTX_free( _client );
   SSL_CTX_free( _server );
}

tls_connection::tls_connection( const tls_context& context, fc::tcp_socket& socket, role r,
                                const std::string& server_name ) :
   _socket( socket ),
   _role( r ),
   _input_buffer( new char[tls_io_buffer_size], [](char* p){ delete[] p; } ),
   _output_buffer( new char[tls_io_buffer_size], [](char* p){ delete[] p; } )
{
   SSL_CTX* ssl_context = r == role::client ? context.client_context() : context.server_context();
   FC_ASSERT( ssl_context, "P2P TLS is off" );
   ERR_clear_error();
   _ssl = SSL_new( ssl_context );
   _input = BIO_new( BIO_s_mem() );
   _output = BIO_new( BIO_s_mem() );
   if( !_ssl || !_input || !_output )
   {
      BIO_free( _input );
      BIO_free( _output );
      SSL_free( _ssl );
      throw_tls_error( "creating a TLS connection" );
   }
   // An empty input BIO means "wait for more", not end of stream
   BIO_set_mem_eof_return( _input, -1 );
   BIO_set_mem_eof_return( _output, -1 );
   SSL_set_bio( _ssl, _input, _output ); // _ssl owns both now
   if( r == role::client )
   {
      SSL_set_connect_state( _ssl );
      if( !server_name.empty() && SSL_set_tlsext_host_name( _ssl, server_name.c_str() ) != 1 )
      {
         SSL_free( _ssl );
         throw_tls_error( "setting the TLS server name " + server_name );
      }
   }
   else
      SSL_set_accept_state( _ssl );
}

tls_connection::~tls_connection()
{
   SSL_free( _ssl );
}

void tls_connection::feed( const char* data, size_t length )
{
   if( length == 0 )
      return;
   _received_anything = true;
   if( BIO_write( _input, data, int(length) ) != int(length) )
      throw_tls_error( "buffering TLS input" );
}

void tls_connection::read_input()
{
   size_t bytes_read = _socket.readsome( _input_buffer, tls_io_buffer_size, 0 );
   if( _role == role::client && !_received_anything )
   {
      // An old node does not know TLS: it answers the ClientHello with its own stcp key
      // (first byte 0x02 or 0x03) without reading anything. A TLS server starts with a
      // handshake record, or with an alert when it rejects us.
      const char first_byte = _input_buffer.get()[0];
      if( first_byte != tls_handshake_record_type && first_byte != tls_alert_record_type )
         FC_THROW_EXCEPTION( p2p_tls_not_supported, "The peer answered TLS with byte ${b}, not TLS",
                             ("b", fc::to_hex( &first_byte, 1 )) );
   }
   feed( _input_buffer.get(), bytes_read );
}

void tls_connection::flush_output()
{
   // One fiber at a time, so that the records of two fibers do not interleave on the wire
   fc::scoped_lock<fc::mutex> lock( _output_mutex );
   for( ;; )
   {
      int bytes = BIO_read( _output, _output_buffer.get(), int(tls_io_buffer_size) );
      if( bytes <= 0 )
         break;
      _socket.write( _output_buffer, size_t(bytes) );
   }
}

void tls_connection::handshake()
{
   for( ;; )
   {
      ERR_clear_error();
      int result = SSL_do_handshake( _ssl );
      int error = result == 1 ? SSL_ERROR_NONE : SSL_get_error( _ssl, result );
      flush_output();
      if( result == 1 )
         return;
      if( error == SSL_ERROR_WANT_READ )
         read_input();
      else
         throw_tls_error( "TLS handshake" );
   }
}

size_t tls_connection::readsome( char* buffer, size_t length )
{
   for( ;; )
   {
      ERR_clear_error();
      int result = SSL_read( _ssl, buffer, int( std::min<size_t>( length, INT32_MAX ) ) );
      int error = result > 0 ? SSL_ERROR_NONE : SSL_get_error( _ssl, result );
      // Reading can produce output, such as the answer to a TLS 1.3 key update
      if( BIO_ctrl_pending( _output ) )
         flush_output();
      if( result > 0 )
         return size_t(result);
      if( error == SSL_ERROR_WANT_READ )
         read_input();
      else if( error == SSL_ERROR_ZERO_RETURN )
         FC_THROW_EXCEPTION( fc::eof_exception, "The peer closed the TLS connection" );
      else
         throw_tls_error( "reading from a TLS connection" );
   }
}

void tls_connection::write( const char* buffer, size_t length )
{
   while( length > 0 )
   {
      ERR_clear_error();
      // The output BIO grows as needed, so a write never waits for the network
      int result = SSL_write( _ssl, buffer, int( std::min<size_t>( length, INT32_MAX ) ) );
      if( result <= 0 )
         throw_tls_error( "writing to a TLS connection" );
      buffer += result;
      length -= size_t(result);
   }
   flush_output();
}

std::string tls_connection::version() const
{
   return SSL_get_version( _ssl );
}

} } // graphene::net
