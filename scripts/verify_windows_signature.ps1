#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Path)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if(-not $IsWindows){throw 'PE Authenticode verification requires Windows.'}
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pin=[IO.File]::ReadAllText((Join-Path $root 'resources/signing/publisher.json'),[Text.Encoding]::UTF8)|ConvertFrom-Json
if($pin.sha256 -notmatch '^[0-9a-f]{64}$' -or $pin.thumbprint -notmatch '^[0-9A-F]{40}$'){throw 'Pinned publisher identity is malformed.'}
Add-Type -AssemblyName System.Security.Cryptography.Pkcs
# Public algorithm: Microsoft PE Format Appendix A (Authenticode PE Image Hash).
# The pinned release profile rejects noncontiguous/overlapping sections and nonterminal
# certificate tables rather than accepting ambiguous or unmeasured PE layouts.
if(-not ('NumeraPinnedAuthenticode' -as [type])){
 $references=@(Get-ChildItem -LiteralPath (Join-Path $PSHOME 'ref') -Filter '*.dll' -File|ForEach-Object {$_.FullName})
 $pkcs=[Security.Cryptography.Pkcs.SignedCms].Assembly.Location
 $references=@($references|Where-Object {[IO.Path]::GetFileName($_) -ne [IO.Path]::GetFileName($pkcs)})+$pkcs
 Add-Type -ReferencedAssemblies ($references|Select-Object -Unique) -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Security.Cryptography.Pkcs;

public static class NumeraPinnedAuthenticode {
    static void Need(bool value, string message) { if (!value) throw new InvalidDataException(message); }
    static void Range(byte[] data, long offset, long length) {
        Need(offset >= 0 && length >= 0 && offset <= data.LongLength - length, "PE structure is truncated or out of bounds.");
    }
    static ushort U16(byte[] data, int offset) { Range(data, offset, 2); return BitConverter.ToUInt16(data, offset); }
    static uint U32(byte[] data, int offset) { Range(data, offset, 4); return BitConverter.ToUInt32(data, offset); }
    static int Position(uint value) { Need(value <= Int32.MaxValue, "PE offset exceeds the supported release profile."); return (int)value; }
    static bool Equal(byte[] left, byte[] right) {
        if (left.Length != right.Length) return false;
        int difference = 0;
        for (int i = 0; i < left.Length; i++) difference |= left[i] ^ right[i];
        return difference == 0;
    }
    sealed class Der {
        readonly byte[] data;
        readonly int end;
        int offset;
        public Der(byte[] bytes) { data=bytes; end=bytes.Length; offset=0; }
        Der(byte[] bytes, int start, int finish) { data=bytes; offset=start; end=finish; }
        public bool Empty { get { return offset == end; } }
        public Der ReadAny() { Need(offset < end, "Missing optional Authenticode DER value."); return Read(data[offset]); }
        public Der Read(int tag) {
            Need(offset < end && data[offset++] == tag, "Authenticode DER tag differs from the required structure.");
            Need(offset < end, "Authenticode DER length is missing.");
            int length=data[offset++];
            if ((length & 128) != 0) {
                int count=length & 127;
                Need(count >= 1 && count <= 4 && count <= end-offset && data[offset] != 0, "Invalid DER length encoding.");
                long total=0;
                for (int i=0; i<count; i++) total=(total<<8)|data[offset++];
                Need(total >= 128 && total <= Int32.MaxValue, "Invalid or oversized DER value.");
                length=(int)total;
            }
            Need(length <= end-offset, "Truncated Authenticode DER content.");
            var child=new Der(data, offset, offset+length);
            offset+=length;
            return child;
        }
        public byte[] Bytes() {
            byte[] result=new byte[end-offset];
            Buffer.BlockCopy(data, offset, result, 0, result.Length);
            offset=end;
            return result;
        }
        public void End() { Need(Empty, "Unexpected trailing Authenticode DER fields."); }
    }
    static byte[] ImageDigest(SignedCms cms) {
        Need(cms.ContentInfo.ContentType.Value == "1.3.6.1.4.1.311.2.1.4", "CMS does not contain Authenticode indirect data.");
        var content=new Der(cms.ContentInfo.Content);
        var indirect=content.Read(0x30); content.End();
        var data=indirect.Read(0x30);
        Need(Equal(data.Read(0x06).Bytes(), new byte[]{0x2b,0x06,0x01,0x04,0x01,0x82,0x37,0x02,0x01,0x0f}), "Authenticode content is not a PE image.");
        // SpcAttributeTypeAndOptionalValue.value is optional and owned by the PE SIP.
        if (!data.Empty) data.ReadAny().Bytes();
        data.End();
        var digest=indirect.Read(0x30); indirect.End();
        var algorithm=digest.Read(0x30);
        Need(Equal(algorithm.Read(0x06).Bytes(), new byte[]{0x60,0x86,0x48,0x01,0x65,0x03,0x04,0x02,0x01}), "Authenticode image digest must be SHA-256.");
        if (!algorithm.Empty) Need(algorithm.Read(0x05).Bytes().Length == 0, "Unexpected SHA-256 parameters.");
        algorithm.End();
        byte[] expected=digest.Read(0x04).Bytes(); digest.End();
        Need(expected.Length == 32, "Authenticode SHA-256 digest has the wrong length.");
        return expected;
    }
    public static void Verify(string path, string sha256, string thumbprint) {
        var file=new FileInfo(path);
        Need(file.Exists && file.Length > 0 && file.Length <= 536870912, "A bounded regular PE release file is required.");
        Need((file.Attributes & FileAttributes.ReparsePoint) == 0, "Linked PE inputs are not accepted.");
        byte[] bytes=File.ReadAllBytes(file.FullName);
        Need(U16(bytes, 0) == 0x5a4d, "Missing DOS image header.");
        int pe=Position(U32(bytes, 0x3c));
        Range(bytes, pe, 24);
        Need(pe >= 64 && U32(bytes, pe) == 0x00004550, "Missing PE image signature.");
        int count=U16(bytes, pe+6), optional=pe+24, optionalSize=U16(bytes, pe+20);
        Need(count > 0 && count <= 96, "Unsupported PE section count.");
        Range(bytes, optional, optionalSize);
        int magic=U16(bytes, optional), directory, numberOffset;
        if (magic == 0x10b) { directory=optional+96; numberOffset=optional+92; }
        else if (magic == 0x20b) { directory=optional+112; numberOffset=optional+108; }
        else throw new InvalidDataException("Unsupported PE optional-header magic.");
        int checksum=optional+64, security=directory+32;
        Need(security+8 <= optional+optionalSize && U32(bytes, numberOffset) > 4, "PE security directory is unavailable.");
        int headers=Position(U32(bytes, optional+60));
        Range(bytes, 0, headers);
        Need(headers >= optional+optionalSize+count*40 && checksum+4 <= security && security+8 <= headers, "Invalid PE header layout.");
        int certificate=Position(U32(bytes, security)), certificateSize=Position(U32(bytes, security+4));
        Need(certificate >= headers && certificate % 8 == 0 && certificateSize >= 8 && certificateSize % 8 == 0, "Invalid aligned PE certificate table.");
        Range(bytes, certificate, certificateSize);
        Need((long)certificate+certificateSize == bytes.LongLength, "Certificate table must terminate the release PE file.");
        int signedLength=Position(U32(bytes, certificate));
        Need(signedLength >= 8 && signedLength <= certificateSize && ((long)signedLength+7)/8*8 == certificateSize, "Only one bounded WIN_CERTIFICATE is permitted.");
        Need(U16(bytes, certificate+4) == 0x0200 && U16(bytes, certificate+6) == 0x0002, "WIN_CERTIFICATE must contain PKCS#7 SignedData.");
        for (int i=certificate+signedLength; i<bytes.Length; i++) Need(bytes[i] == 0, "Nonzero certificate padding is not accepted.");
        var encoded=new byte[signedLength-8];
        Buffer.BlockCopy(bytes, certificate+8, encoded, 0, encoded.Length);
        // Require one complete DER SignedData value, not an accepted prefix with appended bytes.
        var envelope=new Der(encoded); envelope.Read(0x30).Bytes(); envelope.End();
        var cms=new SignedCms(); cms.Decode(encoded);
        Need(cms.SignerInfos.Count == 1, "Exactly one pinned Authenticode signer is required.");
        var signer=cms.SignerInfos[0].Certificate;
        Need(signer != null, "Authenticode signer certificate is missing.");
        using (var hash=SHA256.Create()) {
            Need(String.Equals(Convert.ToHexString(hash.ComputeHash(signer.RawData)), sha256, StringComparison.OrdinalIgnoreCase)
                && String.Equals(signer.Thumbprint, thumbprint, StringComparison.OrdinalIgnoreCase), "Authenticode signer differs from the approved certificate pin.");
        }
        Need(cms.SignerInfos[0].DigestAlgorithm.Value == "2.16.840.1.101.3.4.2.1", "CMS signer digest must be SHA-256.");
        cms.CheckSignature(true); // Cryptographic signature only: no public-root/global trust requirement.
        byte[] expected=ImageDigest(cms);
        var sections=new List<Tuple<int,int>>();
        int sectionTable=optional+optionalSize;
        for (int i=0; i<count; i++) {
            int entry=sectionTable+i*40;
            int size=Position(U32(bytes, entry+16));
            if (size == 0) continue;
            int start=Position(U32(bytes, entry+20));
            Range(bytes, start, size);
            Need(start >= headers && (long)start+size <= certificate, "PE section overlaps header or certificate table.");
            sections.Add(Tuple.Create(start,size));
        }
        sections.Sort((left,right)=>left.Item1.CompareTo(right.Item1));
        using (var hash=IncrementalHash.CreateHash(HashAlgorithmName.SHA256)) {
            hash.AppendData(bytes,0,checksum);
            hash.AppendData(bytes,checksum+4,security-(checksum+4));
            hash.AppendData(bytes,security+8,headers-(security+8));
            int sum=headers;
            foreach (var section in sections) {
                Need(section.Item1 == sum, "Noncontiguous or overlapping raw PE sections are outside the verified release profile.");
                hash.AppendData(bytes,section.Item1,section.Item2);
                sum=checked(sum+section.Item2);
            }
            Need(sum <= certificate, "PE section sum overlaps certificate table.");
            hash.AppendData(bytes,sum,certificate-sum); // Includes overlay/NSIS payload and alignment before certificates.
            Need(Equal(hash.GetHashAndReset(),expected), "Authenticode PE image digest mismatch; file was altered.");
        }
    }
}
'@
}
[NumeraPinnedAuthenticode]::Verify([IO.Path]::GetFullPath($Path),[string]$pin.sha256,[string]$pin.thumbprint)
Write-Verbose 'Pinned Authenticode CMS signature and SHA-256 PE image integrity verified; no public trust installed.'
